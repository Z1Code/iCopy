import io
import json
import os
import sys
from datetime import datetime, timezone

import pytest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import puente_icloud as pi  # noqa: E402
from pyicloud_ipd.exceptions import PyiCloudFailedLoginException  # noqa: E402
from pyicloud_ipd.item_type import AssetItemType  # noqa: E402
from pyicloud_ipd.version_size import AssetVersionSize, LivePhotoVersionSize  # noqa: E402


class V:
    def __init__(self, size, url, type, checksum="ck"):
        self.size = size
        self.url = url
        self.type = type
        self.checksum = checksum


class AssetFalso:
    def __init__(self, id="AbC-123+xyz", nombre="IMG_0001.HEIC", versiones=None,
                 item_type=AssetItemType.IMAGE, campos=None, campos_master=None,
                 fecha=datetime(2026, 8, 10, 23, 37, 18, tzinfo=timezone.utc)):
        self.id = id
        self._nombre = nombre
        self.versions = versiones if versiones is not None else {}
        self.item_type = item_type
        self.item_type_extension = "HEIC"
        self._asset_record = {"fields": campos or {}, "recordChangeTag": "tag1", "recordName": id}
        self._master_record = {"fields": campos_master or {}, "recordName": id}
        self.asset_date = fecha
        self.has_asset_date = True
        self.added_date = fecha

    def calculate_filename(self):
        return self._nombre


class ApiFalsa:
    def __init__(self, requires_2fa=False):
        self.requires_2fa = requires_2fa
        self.requires_2sa = False

    def validate_2fa_code(self, codigo):
        return codigo == "123456"


@pytest.fixture(autouse=True)
def salida(monkeypatch):
    buf = io.StringIO()
    monkeypatch.setattr(pi, "_salida", buf)
    return buf


def lineas(buf):
    return [json.loads(l) for l in buf.getvalue().splitlines()]


def test_live_photo_editada_tiene_tres_partes():
    a = AssetFalso(
        versiones={
            AssetVersionSize.ORIGINAL: V(100, "u1", "public.heic"),
            LivePhotoVersionSize.ORIGINAL: V(50, "u2", "com.apple.quicktime-movie"),
            AssetVersionSize.ADJUSTED: V(80, "u3", "public.jpeg"),
        },
        campos={"resJPEGFullRes": {}},
    )
    j = pi.asset_a_json(a, "PrimarySync")
    assert j["nombre_base"] == "IMG_0001"
    assert j["biblioteca"] == "PrimarySync"
    assert [(p["tipo"], p["extension"], p["tamano"]) for p in j["partes"]] == [
        ("original", "HEIC", 100), ("live", "MOV", 50), ("editada", "JPG", 80)]


def test_sin_edicion_no_incluye_editada():
    a = AssetFalso(versiones={
        AssetVersionSize.ORIGINAL: V(100, "u1", "public.heic"),
        AssetVersionSize.ADJUSTED: V(100, "u1", "public.heic"),
    })
    assert [p["tipo"] for p in pi.asset_a_json(a, "PrimarySync")["partes"]] == ["original"]


def test_video_solo_original():
    a = AssetFalso(nombre="IMG_0002.MOV", item_type=AssetItemType.MOVIE, versiones={
        AssetVersionSize.ORIGINAL: V(900, "u", "com.apple.quicktime-movie"),
        LivePhotoVersionSize.ORIGINAL: V(1, "x", "com.apple.quicktime-movie"),
    })
    partes = pi.asset_a_json(a, "PrimarySync")["partes"]
    assert [(p["tipo"], p["extension"]) for p in partes] == [("original", "MOV")]


def test_sin_original_se_omite():
    assert pi.asset_a_json(AssetFalso(versiones={}), "PrimarySync") is None


def test_sin_nombre_usa_respaldo():
    a = AssetFalso(nombre=None, versiones={AssetVersionSize.ORIGINAL: V(1, "u", "public.heic")})
    assert pi.asset_a_json(a, "PrimarySync")["nombre_base"] == "AbC_123_xyz"


def test_extension():
    assert pi.extension_de("public.webp", "foto.webp") == "WEBP"
    assert pi.extension_de("public.heic", "x.jpg") == "HEIC"
    assert pi.extension_de("raro", "sinext") == "BIN"


def test_fecha_local_con_desfase():
    a = AssetFalso(versiones={AssetVersionSize.ORIGINAL: V(1, "u", "public.heic")})
    f = datetime.fromisoformat(pi.asset_a_json(a, "PrimarySync")["fecha"])
    assert f.utcoffset() is not None
    assert f == a.asset_date


def test_sesion_sin_token_es_caducada():
    def fabrica(_pw):
        raise PyiCloudFailedLoginException("sin sesión")
    p = pi.Puente("yo@x.com", "c", fabrica=fabrica)
    with pytest.raises(pi.SesionCaducada):
        p.manejar({"cmd": "sesion"})


def test_sesion_que_pide_2fa_es_caducada():
    p = pi.Puente("yo@x.com", "c", fabrica=lambda _pw: ApiFalsa(requires_2fa=True))
    with pytest.raises(pi.SesionCaducada):
        p.manejar({"cmd": "sesion"})


def test_login_y_codigo(salida):
    api = ApiFalsa(requires_2fa=True)
    p = pi.Puente("yo@x.com", "c", fabrica=lambda _pw: api)
    p.manejar({"cmd": "login", "password": "x"})
    p.manejar({"cmd": "codigo_2fa", "codigo": "000000"})
    p.manejar({"cmd": "codigo_2fa", "codigo": "123456"})
    assert lineas(salida) == [
        {"ok": True, "requiere_2fa": True},
        {"ok": False, "error": "codigo_incorrecto"},
        {"ok": True},
    ]


def test_login_con_contrasena_mala():
    def fabrica(_pw):
        raise PyiCloudFailedLoginException("mala")
    p = pi.Puente("yo@x.com", "c", fabrica=fabrica)
    with pytest.raises(pi.Credenciales):
        p.manejar({"cmd": "login", "password": "x"})


def test_borrar_solo_lo_listado_y_personal(salida):
    borrados = []
    p = pi.Puente("yo@x.com", "c", fabrica=lambda _pw: ApiFalsa(),
                  borrador=lambda lib, a: borrados.append(a.id))
    p.cache = {
        "A1": ("PrimarySync", "lib", AssetFalso(id="A1")),
        "S1": ("SharedSync-1", "lib2", AssetFalso(id="S1")),
    }
    p.manejar({"cmd": "borrar", "ids": ["A1", "S1", "ZZ"]})
    assert borrados == ["A1"]
    assert lineas(salida) == [{
        "ok": True, "borrados": ["A1"],
        "fallidos": [{"id": "S1", "error": "biblioteca_compartida"},
                     {"id": "ZZ", "error": "no_listado"}],
    }]


def test_listar_emite_assets_avisos_y_total(salida):
    class Lib:
        def __init__(self, assets):
            self.all = assets

    class Rota:
        id = "A3"

        @property
        def versions(self):
            raise ValueError("roto")

    buena = AssetFalso(id="A1", versiones={AssetVersionSize.ORIGINAL: V(1, "u", "public.heic")})
    sin_original = AssetFalso(id="A2", versiones={})
    p = pi.Puente("yo@x.com", "c", fabrica=lambda _pw: ApiFalsa())
    p.bibliotecas = lambda: [("PrimarySync", Lib([buena, sin_original, Rota()]))]
    p.manejar({"cmd": "listar"})
    ls = lineas(salida)
    assert ls[0]["asset"]["id"] == "A1"
    assert "aviso" in ls[1]
    assert ls[-1] == {"ok": True, "total": 1, "omitidos": 2}
    assert set(p.cache) == {"A1"}


def _res(size, url, ck="k"):
    return {"value": {"size": size, "downloadURL": url, "fileChecksum": ck}}


def test_raw_jpeg_copia_la_alternativa():
    a = AssetFalso(versiones={
        AssetVersionSize.ORIGINAL: V(100, "u1", "public.jpeg"),
        AssetVersionSize.ALTERNATIVE: V(300, "u2", "com.adobe.raw-image"),
    }, campos_master={"resOriginalRes": {}, "resOriginalAltRes": {}})
    j = pi.asset_a_json(a, "PrimarySync")
    assert [(p["tipo"], p["extension"], p["tamano"], p["url"]) for p in j["partes"]] == [
        ("original", "JPG", 100, "u1"), ("alternativa", "DNG", 300, "u2")]
    assert j["liberable"] is True


def test_video_editado_copia_el_render():
    a = AssetFalso(nombre="IMG_0002.MOV", item_type=AssetItemType.MOVIE, versiones={
        AssetVersionSize.ORIGINAL: V(900, "u", "com.apple.quicktime-movie"),
    }, campos={"resVidFullRes": _res(700, "uv", "ckv"),
               "resVidFullFileType": {"value": "com.apple.quicktime-movie"},
               "resVidMedRes": {}, "resVidSmallRes": {}, "resJPEGThumbRes": {}},
       campos_master={"resOriginalRes": {}})
    j = pi.asset_a_json(a, "PrimarySync")
    assert [(p["tipo"], p["extension"], p["tamano"], p["url"], p["checksum"]) for p in j["partes"]] == [
        ("original", "MOV", 900, "u", "ck"), ("editada", "MOV", 700, "uv", "ckv")]
    assert j["liberable"] is True


def test_derivados_no_impiden_liberar():
    a = AssetFalso(versiones={AssetVersionSize.ORIGINAL: V(100, "u1", "public.heic")},
                   campos={"resJPEGMedRes": {}, "resJPEGThumbRes": {}, "resJPEGLargeRes": {}},
                   campos_master={"resOriginalRes": {}, "resJPEGThumbRes": {}})
    assert pi.asset_a_json(a, "PrimarySync")["liberable"] is True


def test_datos_desconocidos_no_son_liberables():
    for campos, master in [({"resSidecarRes": {}}, {}),
                           ({}, {"resAlgoNuevoRes": {}}),
                           ({"resVidFullRes": {}}, {})]:  # vídeo editado de una Live Photo: no se copia
        a = AssetFalso(versiones={AssetVersionSize.ORIGINAL: V(100, "u1", "public.heic")},
                       campos=campos, campos_master=master)
        assert pi.asset_a_json(a, "PrimarySync")["liberable"] is False, (campos, master)


class Respuesta:
    def __init__(self, cuerpo, status_code=200, ok=None):
        self._cuerpo = cuerpo
        self.status_code = status_code
        self.ok = (status_code < 400) if ok is None else ok

    def json(self):
        if isinstance(self._cuerpo, Exception):
            raise self._cuerpo
        return self._cuerpo


class SesionFalsa:
    def __init__(self, respuesta):
        self.respuesta = respuesta
        self.envios = []

    def post(self, url, data=None, headers=None):
        self.envios.append(json.loads(data))
        return self.respuesta


class LibFalsa:
    def __init__(self, respuesta):
        self.service_endpoint = "https://x"
        self.params = {"a": "1"}
        self.zone_id = {"zoneName": "PrimarySync"}
        self.session = SesionFalsa(respuesta)


def _borrar_con(respuesta, id="A1"):
    lib = LibFalsa(respuesta)
    return pi.borrar_en_icloud(lib, AssetFalso(id=id)), lib


def test_borrar_en_icloud_exito_solo_con_el_registro_pedido():
    r, lib = _borrar_con(Respuesta({"records": [{"recordName": "A1", "fields": {"isDeleted": {"value": 1}}}]}))
    assert r is None
    assert lib.session.envios[0]["operations"][0]["record"]["recordName"] == "A1"


def test_borrar_en_icloud_respuestas_dudosas_no_cuentan():
    casos = [
        Respuesta({}),                                            # sin registros
        Respuesta({"records": []}),
        Respuesta({"records": [{"recordName": "OTRO"}]}),          # otro registro
        Respuesta(ValueError("no json")),                         # cuerpo ilegible
        Respuesta({"records": [{"recordName": "A1"}]}, status_code=500),  # HTTP de error
        Respuesta({"records": [{"recordName": "A1"}]}, ok=False),
        Respuesta(["no", "es", "dict"]),
    ]
    for c in casos:
        assert _borrar_con(c)[0] == "respuesta_desconocida", c._cuerpo


def test_borrar_en_icloud_devuelve_el_error_del_servidor():
    r, _ = _borrar_con(Respuesta({"records": [{"recordName": "A1", "serverErrorCode": "CONFLICT"}]}))
    assert r == "CONFLICT"
