"""puente-icloud: puente JSON por líneas entre icopia (Rust) e iCloud Fotos.

Usa la librería pyicloud_ipd de icloudpd v1.33.0. Una petición JSON por línea en stdin;
respuestas JSON por línea en stdout. Todo lo demás (logs) va a stderr.

Peticiones:
  {"cmd":"sesion"}                       -> {"ok":true}
  {"cmd":"login","password":"..."}       -> {"ok":true,"requiere_2fa":bool}
  {"cmd":"codigo_2fa","codigo":"123456"} -> {"ok":true} | {"ok":false,"error":"codigo_incorrecto"}
  {"cmd":"listar"}                       -> {"asset":{...}}* {"aviso":"..."}* {"ok":true,"total":N,"omitidos":M}
  {"cmd":"borrar","ids":[...]}           -> {"ok":true,"borrados":[...],"fallidos":[{"id":..,"error":..}]}
Errores: {"ok":false,"error":"sesion_caducada"|"credenciales"|"otro","detalle":"..."}
"""
from __future__ import annotations

import argparse
import io
import json
import logging
import os
import sys
import traceback
import urllib.parse
from typing import Any, Callable, Dict, List, Optional, Tuple

from pyicloud_ipd.asset_version import ITEM_TYPE_EXTENSIONS, AssetVersion
from pyicloud_ipd.base import PyiCloudService
from pyicloud_ipd.exceptions import PyiCloudAPIResponseException, PyiCloudFailedLoginException
from pyicloud_ipd.item_type import AssetItemType
from pyicloud_ipd.services.photos import filename_with_fallback
from pyicloud_ipd.version_size import AssetVersionSize, LivePhotoVersionSize
from tzlocal import get_localzone

BIBLIOTECA_PERSONAL = "PrimarySync"

_salida: Any = None  # se fija en main(); las pruebas lo sustituyen


class SesionCaducada(Exception):
    pass


class Credenciales(Exception):
    pass


def responder(obj: Dict[str, Any]) -> None:
    _salida.write(json.dumps(obj) + "\n")
    _salida.flush()


def extension_de(uti: str, nombre: str) -> str:
    if uti in ITEM_TYPE_EXTENSIONS:
        return ITEM_TYPE_EXTENSIONS[uti]
    ext = os.path.splitext(nombre)[1][1:]
    return ext.upper() if ext else "BIN"


def fecha_de(asset: Any) -> str:
    if asset.has_asset_date:
        dt = asset.asset_date
    else:
        try:
            dt = asset.added_date
        except (KeyError, TypeError, ValueError):
            dt = asset.asset_date
    return dt.astimezone(get_localzone()).isoformat()


def _parte(tipo: str, version: Any, extension: str) -> Dict[str, Any]:
    return {"tipo": tipo, "tamano": int(version.size), "url": version.url,
            "extension": extension, "checksum": version.checksum or ""}


# Campos res*Res que son solo copias reducidas (miniaturas, vistas previas) y se regeneran.
DERIVADOS = {"resJPEGLarge", "resJPEGMed", "resJPEGThumb", "resVidMed", "resVidSmall"}


def _campos(registro: Any) -> Dict[str, Any]:
    if not isinstance(registro, dict):
        return {}
    return registro.get("fields") or {}


def _prefijos_res(asset: Any) -> set:
    """Prefijos de todos los recursos (campos 'res...Res') del asset y su master."""
    prefijos = set()
    for registro in (getattr(asset, "_asset_record", None), getattr(asset, "_master_record", None)):
        for clave in _campos(registro):
            if clave.startswith("res") and clave.endswith("Res") and len(clave) > 6:
                prefijos.add(clave[:-3])
    return prefijos


def _version_de_campos(campos: Dict[str, Any], prefijo: str) -> Optional[AssetVersion]:
    res = (campos.get(f"{prefijo}Res") or {}).get("value")
    tipo = (campos.get(f"{prefijo}FileType") or {}).get("value")
    if not res or not tipo:
        return None
    return AssetVersion(res["size"], res["downloadURL"], tipo, res.get("fileChecksum"))


def asset_a_json(asset: Any, biblioteca: str) -> Optional[Dict[str, Any]]:
    versiones = asset.versions
    original = versiones.get(AssetVersionSize.ORIGINAL)
    if original is None:
        return None
    nombre = filename_with_fallback(asset.id, asset.item_type_extension)(asset.calculate_filename())
    partes = [_parte("original", original, extension_de(original.type, nombre))]
    copiados = {"resOriginal"}
    campos_asset = _campos(asset._asset_record)
    if asset.item_type == AssetItemType.MOVIE:
        if "resVidFullRes" in campos_asset:
            editada = _version_de_campos(campos_asset, "resVidFull")
            if editada is not None:
                partes.append(_parte("editada", editada, extension_de(editada.type, nombre)))
                copiados.add("resVidFull")
    else:
        live = versiones.get(LivePhotoVersionSize.ORIGINAL)
        if live is not None:
            partes.append(_parte("live", live, "MOV"))
            copiados.add("resOriginalVidCompl")
        alternativa = versiones.get(AssetVersionSize.ALTERNATIVE)
        if alternativa is not None:
            partes.append(_parte("alternativa", alternativa, extension_de(alternativa.type, nombre)))
            copiados.add("resOriginalAlt")
        if "resJPEGFullRes" in campos_asset:
            editada = versiones.get(AssetVersionSize.ADJUSTED)
            if editada is not None:
                partes.append(_parte("editada", editada, extension_de(editada.type, nombre)))
                copiados.add("resJPEGFull")
    # Si iCloud guarda algo que no copiamos (y no es una miniatura), borrar el asset lo perdería.
    liberable = not (_prefijos_res(asset) - copiados - DERIVADOS)
    return {"id": asset.id, "biblioteca": biblioteca,
            "nombre_base": os.path.splitext(nombre)[0], "fecha": fecha_de(asset),
            "liberable": liberable, "partes": partes}


def borrar_en_icloud(lib: Any, asset: Any) -> Optional[str]:
    """Manda el asset a 'Eliminado recientemente'. Devuelve None si fue bien o un código de error."""
    url = f"{lib.service_endpoint}/records/modify?{urllib.parse.urlencode(lib.params)}"
    datos = json.dumps({
        "atomic": True,
        "desiredKeys": ["isDeleted"],
        "operations": [{
            "operationType": "update",
            "record": {
                "fields": {"isDeleted": {"value": 1}},
                "recordChangeTag": asset._asset_record["recordChangeTag"],
                "recordName": asset._asset_record["recordName"],
                "recordType": "CPLAsset",
            },
        }],
        "zoneID": lib.zone_id,
    })
    try:
        respuesta = lib.session.post(url, data=datos, headers={"Content-type": "application/json"})
    except PyiCloudAPIResponseException as e:
        if e.code == "TRY_AGAIN_LATER":
            return "reintentar"
        raise
    # Solo cuenta como borrado si iCloud confirma explícitamente ESE registro sin error.
    estado = getattr(respuesta, "status_code", None)
    if getattr(respuesta, "ok", True) is False or (isinstance(estado, int) and estado >= 400):
        return "respuesta_desconocida"
    try:
        cuerpo = respuesta.json()
    except ValueError:
        return "respuesta_desconocida"
    registros = cuerpo.get("records") if isinstance(cuerpo, dict) else None
    if not isinstance(registros, list):
        return "respuesta_desconocida"
    nombre = asset._asset_record["recordName"]
    for registro in registros:
        if isinstance(registro, dict) and registro.get("recordName") == nombre:
            if "serverErrorCode" in registro:
                return str(registro["serverErrorCode"])
            return None
    return "respuesta_desconocida"


def es_error_de_sesion(e: Exception) -> bool:
    codigo = str(getattr(e, "code", "") or "")
    return codigo in ("401", "421", "AUTHENTICATION_FAILED") or "Authentication required" in str(e)


class Puente:
    def __init__(self, apple_id: str, cookies: str,
                 fabrica: Optional[Callable[[Optional[str]], Any]] = None,
                 borrador: Optional[Callable[[Any, Any], Optional[str]]] = None) -> None:
        self.apple_id = apple_id
        self.cookies = cookies
        self.api: Any = None
        self.cache: Dict[str, Tuple[str, Any, Any]] = {}
        self._fabrica = fabrica or self._fabrica_real
        self._borrador = borrador or borrar_en_icloud

    def _fabrica_real(self, password: Optional[str]) -> Any:
        return PyiCloudService("com", self.apple_id, lambda: password, cookie_directory=self.cookies)

    def conectar(self, password: Optional[str] = None) -> Any:
        try:
            return self._fabrica(password)
        except PyiCloudFailedLoginException as e:
            if password is None:
                raise SesionCaducada() from e
            raise Credenciales(str(e)) from e

    def api_lista(self) -> Any:
        if self.api is None:
            api = self.conectar()
            if api.requires_2fa or api.requires_2sa:
                raise SesionCaducada()
            self.api = api
        return self.api

    def bibliotecas(self) -> List[Tuple[str, Any]]:
        fotos = self.api_lista().photos
        libs = [(n, l) for n, l in fotos.private_libraries.items()
                if n == BIBLIOTECA_PERSONAL or n.startswith("SharedSync-")]
        if not any(n == BIBLIOTECA_PERSONAL for n, _ in libs):
            libs.insert(0, (BIBLIOTECA_PERSONAL, fotos))
        return libs

    def manejar(self, p: Dict[str, Any]) -> None:
        cmd = p.get("cmd")
        if cmd == "sesion":
            self.api_lista()
            responder({"ok": True})
        elif cmd == "login":
            self.api = None
            self.api = self.conectar(str(p["password"]))
            responder({"ok": True,
                       "requiere_2fa": bool(self.api.requires_2fa or self.api.requires_2sa)})
        elif cmd == "codigo_2fa":
            if self.api is None:
                raise SesionCaducada()
            if self.api.validate_2fa_code(str(p["codigo"])):
                responder({"ok": True})
            else:
                responder({"ok": False, "error": "codigo_incorrecto"})
        elif cmd == "listar":
            self.listar()
        elif cmd == "borrar":
            self.borrar(list(p.get("ids", [])))
        else:
            responder({"ok": False, "error": "otro", "detalle": f"comando desconocido: {cmd}"})

    def listar(self) -> None:
        self.cache = {}
        total = 0
        omitidos = 0
        for nombre, lib in self.bibliotecas():
            for asset in lib.all:
                try:
                    j = asset_a_json(asset, nombre)
                except Exception as e:  # un asset raro no debe tumbar el listado
                    omitidos += 1
                    logging.exception("asset omitido")
                    responder({"aviso": f"asset {getattr(asset, 'id', '?')} omitido: {e}"})
                    continue
                if j is None:
                    omitidos += 1
                    continue
                self.cache[asset.id] = (nombre, lib, asset)
                total += 1
                responder({"asset": j})
        responder({"ok": True, "total": total, "omitidos": omitidos})

    def borrar(self, ids: List[str]) -> None:
        borrados: List[str] = []
        fallidos: List[Dict[str, str]] = []
        for i in ids:
            entrada = self.cache.get(i)
            if entrada is None:
                fallidos.append({"id": i, "error": "no_listado"})
                continue
            biblioteca, lib, asset = entrada
            if biblioteca != BIBLIOTECA_PERSONAL:
                fallidos.append({"id": i, "error": "biblioteca_compartida"})
                continue
            error = self._borrador(lib, asset)
            if error:
                fallidos.append({"id": i, "error": error})
            else:
                borrados.append(i)
        responder({"ok": True, "borrados": borrados, "fallidos": fallidos})


def main(argv: Optional[List[str]] = None) -> None:
    global _salida
    ap = argparse.ArgumentParser()
    ap.add_argument("--cookies", required=True)
    ap.add_argument("--apple-id", required=True)
    args = ap.parse_args(argv)
    _salida = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", newline="\n")
    sys.stdout = sys.stderr  # nada de la librería puede ensuciar el protocolo
    logging.basicConfig(stream=sys.stderr, level=logging.INFO,
                        format="%(asctime)s %(levelname)s %(message)s")
    puente = Puente(args.apple_id, args.cookies)
    for linea in io.TextIOWrapper(sys.stdin.buffer, encoding="utf-8"):
        linea = linea.strip()
        if not linea:
            continue
        try:
            puente.manejar(json.loads(linea))
        except SesionCaducada:
            responder({"ok": False, "error": "sesion_caducada"})
        except Credenciales as e:
            responder({"ok": False, "error": "credenciales", "detalle": str(e)})
        except PyiCloudAPIResponseException as e:
            if es_error_de_sesion(e):
                responder({"ok": False, "error": "sesion_caducada"})
            else:
                traceback.print_exc()
                responder({"ok": False, "error": "otro", "detalle": str(e)})
        except Exception as e:
            traceback.print_exc()
            responder({"ok": False, "error": "otro", "detalle": f"{type(e).__name__}: {e}"})


if __name__ == "__main__":
    main()
