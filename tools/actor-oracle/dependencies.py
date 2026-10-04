"""Second independent reader for actor_dependencies, augmenting the native oracle.

Reads original plugin bytes and BSA metadata directly with Python's standard
library. No production decoder, archive backend, retail engine or Rust report is
used. Tarjan's algorithm is independent of the production Kosaraju helper.
"""
import argparse
import collections
import ctypes
import hashlib
import json
import os
import pathlib
import struct
import zlib

MIB = 1024 * 1024
KINDS = {"NPC_", "CREA", "RACE", "HDPT", "HAIR", "EYES"}
GRAPH_KINDS = {"CONT", "NPC_", "CREA", "LVLI", "LVLC", "LVLN"}
VERSIONS = {"NPC_": {14, 15}, "CREA": {9, 11, 13, 14, 15}, "RACE": {15}, "HDPT": {15}, "HAIR": {15}, "EYES": {3, 14, 15}}


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def key_json(key):
    return None if key is None else {"profile": "nv-original", "origin_plugin": key[0], "local_id": key[1]}


def key_tuple(value):
    return None if value is None else (value["origin_plugin"], value["local_id"])


def plugin_name(name):
    require(name and name not in {".", ".."} and len(name) <= 4096 and name.isascii() and all(ord(c) >= 32 and c not in "/\\:" for c in name), "plugin name")
    return name.lower()


def terminated(raw):
    require(raw.endswith(b"\0"), "string is not NUL terminated")
    require(b"\0" not in raw[:-1], "embedded NUL in string")
    return raw[:-1]


def fields(body):
    cursor, extended, count = 0, None, 0
    while cursor < len(body):
        require(cursor + 6 <= len(body), "dependency field header")
        start = cursor
        tag, length = struct.unpack_from("<4sH", body, cursor)
        cursor += 6
        if tag == b"XXXX":
            require(length == 4 and extended is None and cursor + 4 <= len(body), "dependency extended prefix")
            extended = struct.unpack_from("<I", body, cursor)[0]
            cursor += 4
            continue
        length = length if extended is None else extended
        extended = None
        require(cursor + length <= len(body), "dependency field extent")
        require(count < 4_000_000, "dependency physical field budget")
        count += 1
        yield tag.decode("latin1"), start, body[cursor:cursor + length]
        cursor += length
    require(extended is None, "dependency orphan extended prefix")


def frame_offsets(data):
    at = 0
    while at < len(data):
        end = data.find(b"\0", at)
        require(end >= at, "unterminated dependency string frame")
        yield at, end
        at = end + 1


class Source:
    def __init__(self, path):
        self.path = path
        self.lock = None
        self.stream = None
        if os.name == "nt":
            self.api = ctypes.WinDLL("kernel32", use_last_error=True)
            self.api.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
            self.api.CreateFileW.restype = ctypes.c_void_p
            self.api.CloseHandle.argtypes = [ctypes.c_void_p]
            self.lock = self.api.CreateFileW(str(path), 0x80000000, 1, None, 3, 0x80, None)
            require(self.lock != ctypes.c_void_p(-1).value, "source read lock")
        try:
            self.stream = path.open("rb")
            self.size = os.fstat(self.stream.fileno()).st_size
            self.sha256 = hashlib.file_digest(self.stream, "sha256").hexdigest()
        except BaseException:
            self.close()
            raise

    def close(self):
        if self.stream is not None:
            self.stream.close()
            self.stream = None
        if self.lock is not None:
            self.api.CloseHandle(self.lock)
            self.lock = None

    def read(self, offset, size):
        require(0 <= offset <= self.size and 0 <= size <= self.size - offset, "source extent")
        self.stream.seek(offset)
        value = self.stream.read(size)
        require(len(value) == size, "source truncated read")
        return value

    def body(self, entry, maximum):
        maximum = min(maximum, 64 * MIB)
        size, offset, flags = entry["stored_size"], entry["offset"] + 24, entry["flags"]
        require(size <= maximum, "dependency stored byte budget")
        if flags & 0x40000:
            require(size >= 4, "compressed dependency length prefix")
            expected = struct.unpack("<I", self.read(offset, 4))[0]
            require(expected <= maximum, "dependency decompression budget")
            compressed = self.read(offset + 4, size - 4)
            decoder = zlib.decompressobj()
            result = decoder.decompress(compressed, expected + 1)
            require(len(result) == expected and decoder.eof and not decoder.unconsumed_tail and not decoder.unused_data, "dependency strict compressed frame")
            return result
        return self.read(offset, size)


class Reader:
    def __init__(self, data, names, native, guard):
        self.data, self.guard = data, guard
        self.sources, self.names, self.masters, self.winners = [], names, [], {}
        self.cache, self.cached_bytes = {}, 0
        self.archives, self.mounts = [], collections.defaultdict(list)
        try:
            receipts = []
            for index, name in enumerate(names):
                guard()
                plugin_name(name)
                require(plugin_name(name) not in [plugin_name(n) for n in names[:index]], "duplicate source")
                source = Source(data / name)
                self.sources.append(source)
                receipts.append({"source_name": name, "source_bytes": source.size, "source_sha256": source.sha256})
                raw = source.read(0, 24)
                header = self.header(raw, 0)
                require(header["kind"] == list(b"TES4"), "plugin TES4 header")
                masters = [plugin_name(terminated(value).decode("ascii")) for tag, _, value in fields(source.body(header, 64 * MIB)) if tag == "MAST"]
                require(all(m in [plugin_name(n) for n in names[:index]] for m in masters), "source master order")
                self.masters.append(masters)
                cursor, ends = 24 + header["stored_size"], [source.size]
                local = set()
                while cursor < source.size:
                    if len(self.winners) % 8192 == 0:
                        guard()
                    while cursor == ends[-1] and len(ends) > 1:
                        ends.pop()
                    require(cursor + 24 <= ends[-1], "plugin nested header")
                    raw = source.read(cursor, 24)
                    entry = self.header(raw, cursor)
                    if raw[:4] == b"GRUP":
                        require(entry["stored_size"] >= 24 and cursor + entry["stored_size"] <= ends[-1] and len(ends) <= 64, "plugin group extent/depth")
                        ends.append(cursor + entry["stored_size"])
                        cursor += 24
                        continue
                    require(cursor + 24 + entry["stored_size"] <= ends[-1], "plugin record extent")
                    require(entry["form_id"] != 0, "plugin zero definition")
                    key = self.resolve(index, entry["form_id"])
                    require(key not in local and len(self.winners) < 1_000_000, "plugin duplicate identity/header budget")
                    local.add(key)
                    entry["source"] = index
                    entry["kind_name"] = raw[:4].decode("latin1")
                    self.winners[key] = entry
                    cursor += 24 + entry["stored_size"]
                require(cursor == source.size and all(end == cursor for end in ends), "plugin unfinished group")
            require(receipts == native["sources"], "native/raw dependency source cohorts differ")
            require(len(self.winners) == native["metadata"]["winning_definitions"], "native/raw winner counts differ")
        except BaseException:
            self.close()
            raise

    def close(self):
        for source in self.sources + self.archives:
            source.close()

    @staticmethod
    def header(raw, offset):
        kind, size, flags, form, revision, version, trailing = struct.unpack("<4sIIIIH2s", raw)
        return {"kind": list(kind), "offset": offset, "stored_size": size, "flags": flags, "form_id": form, "revision": list(raw[16:20]), "version": version, "trailing_bytes": list(trailing)}

    def resolve(self, source, raw):
        if raw == 0:
            return None
        selector = raw >> 24
        return (self.masters[source][selector] if selector < len(self.masters[source]) else plugin_name(self.names[source]), raw & 0xFFFFFF)

    def binding(self, source, raw):
        key = self.resolve(source, raw)
        entry = self.winners.get(key)
        status = "null" if key is None else "missing" if entry is None else "deleted" if entry["flags"] & 0x20 else "defined"
        target = None if entry is None else {"kind": entry["kind"], "source_plugin": self.names[entry["source"]], "record_file_offset": entry["offset"], "record_flags": entry["flags"]}
        return {"raw_form": raw, "key": key_json(key), "status": status, "target": target}

    def payload(self, key, maximum):
        if key in self.cache:
            raw = self.cache[key]
            require(len(raw) <= maximum and self.winners[key]["stored_size"] <= maximum, "dependency cached body budget")
            return raw
        entry = self.winners[key]
        require(not entry["flags"] & 0x20, "tombstone body access")
        raw = self.sources[entry["source"]].body(entry, min(maximum, 640 * MIB - self.cached_bytes))
        self.cache[key] = raw
        self.cached_bytes += len(raw)
        return raw

    def graph(self):
        keys = [key for key, e in sorted(self.winners.items()) if e["kind_name"] in GRAPH_KINDS]
        require(len(keys) <= 131072, "inventory graph node budget")
        nodes = [{"key": key_json(key), "kind": self.winners[key]["kind"], "deleted": bool(self.winners[key]["flags"] & 0x20)} for key in keys]
        edges, total, visits = [], 0, 0
        for key in keys:
            self.guard()
            entry = self.winners[key]
            if entry["flags"] & 0x20:
                continue
            raw = self.payload(key, 384 * MIB - total)
            total += len(raw)
            for tag, at, data in fields(raw):
                visits += 1
                require(visits <= 4_000_000, "inventory graph field budget")
                role, word_at, exact = None, 0, None
                kind = entry["kind_name"]
                if tag == "CNTO" and kind in {"CONT", "NPC_", "CREA"}:
                    role, exact = "base-item", {8}
                elif tag == "TPLT" and kind in {"NPC_", "CREA"}:
                    role, exact = "actor-template", {4}
                elif tag == "LVLO" and kind in {"LVLI", "LVLC", "LVLN"}:
                    role, word_at, exact = "leveled-entry", 4, {8, 10, 12}
                if role is None:
                    continue
                require(len(data) in exact, "inventory graph field shape")
                require(len(edges) < 2_000_000, "inventory graph edge budget")
                binding = self.binding(entry["source"], struct.unpack_from("<I", data, word_at)[0])
                domain = {"NPC_", "LVLN"} if role == "actor-template" and kind == "NPC_" else {"CREA", "LVLC"} if role == "actor-template" or (role == "leveled-entry" and kind == "LVLC") else {"NPC_", "LVLN"} if role == "leveled-entry" and kind == "LVLN" else {"ARMO", "AMMO", "MISC", "WEAP", "BOOK", "LVLI", "KEYM", "ALCH", "NOTE", "IMOD", "CMNY", "CCRD", "LIGH", "CHIP"}
                allowed = None if binding["target"] is None else bytes(binding["target"]["kind"]).decode("ascii") in domain
                edges.append({"source": key_json(key), "field_decoded_offset": at, "role": role, "target": binding["key"], "status": binding["status"], "schema_kind_allowed": allowed})
        edges.sort(key=lambda e: (key_tuple(e["source"]), e["field_decoded_offset"], e["role"]))
        positions = {key: i for i, key in enumerate(keys)}
        children = [[] for _ in keys]
        counts = {"nodes": len(keys), "edges": len(edges), "internal_edges": 0, "terminal_edges": 0, "unresolved_edges": 0, "schema_mismatches": sum(e["schema_kind_allowed"] is False for e in edges)}
        for edge in edges:
            target = key_tuple(edge["target"])
            if edge["status"] != "defined":
                counts["unresolved_edges"] += 1
            elif target in positions:
                counts["internal_edges"] += 1
                children[positions[key_tuple(edge["source"])]].append(positions[target])
            else:
                counts["terminal_edges"] += 1
        groups = cycles(children)
        counts.update(cyclic_components=len(groups), cyclic_nodes=sum(map(len, groups)))
        return {"nodes": nodes, "edges": edges, "cycles": [[key_json(keys[i]) for i in group] for group in groups], "counts": counts}

    def definitions(self):
        counts = {name: 0 for name in ["records", "deleted_records", "decoded_bytes", "fields", "marker_fields", "path_fields", "strings", "empty_strings", "path_bytes", "link_fields", "bindings", "race_links", "source_findings"]}
        counts.update(record_kinds={}, record_versions={})
        definitions = {}
        for key, entry in sorted(self.winners.items()):
            kind = entry["kind_name"]
            if kind not in KINDS:
                continue
            self.guard()
            require(counts["records"] < 65536, "dependency record budget")
            counts["records"] += 1
            counts["record_kinds"][kind] = counts["record_kinds"].get(kind, 0) + 1
            deleted = bool(entry["flags"] & 0x20)
            definition = {"key": key_json(key), "source": {"plugin": self.names[entry["source"]], "sha256": self.sources[entry["source"]].sha256, "record_file_offset": entry["offset"], "record_flags": entry["flags"], "decoded_record_sha256": None}, "header": {name: value for name, value in entry.items() if name not in {"source", "kind_name"}}, "deleted": deleted, "fields": [], "race_links": [], "findings": []}
            definitions[key] = definition
            if deleted:
                counts["deleted_records"] += 1
                continue
            require(entry["version"] in VERSIONS[kind], "unsupported dependency record version")
            raw = self.payload(key, 256 * MIB - counts["decoded_bytes"])
            counts["decoded_bytes"] += len(raw)
            definition["source"]["decoded_record_sha256"] = hashlib.sha256(raw).hexdigest()
            version = f'{kind}:{entry["version"]}'
            counts["record_versions"][version] = counts["record_versions"].get(version, 0) + 1
            context = {"region": None, "sex": None, "part": None}
            seen = collections.Counter()
            for tag, at, data in fields(raw):
                require(counts["fields"] < 2_000_000, "dependency field budget")
                field_index = len(definition["fields"])
                value, singleton = {"kind": "opaque"}, None
                if kind == "RACE":
                    if tag in {"HNAM", "ENAM", "FGGS", "FGGA", "FGTS"}:
                        context = {"region": None, "sex": None, "part": None}
                    if tag in {"NAM0", "NAM1", "MNAM", "FNAM", "INDX"}:
                        require(len(data) == (4 if tag == "INDX" else 0), "race marker shape")
                        marker = {"field_index": field_index, "decoded_offset": at, "kind": list(tag.encode("ascii")), "raw_index": struct.unpack("<I", data)[0] if tag == "INDX" else None}
                        if tag in {"NAM0", "NAM1"}:
                            context = {"region": marker, "sex": None, "part": None}
                        elif tag in {"MNAM", "FNAM"} and context["region"] is not None:
                            context = dict(context, sex=marker, part=None)
                        elif tag == "INDX" and context["region"] is not None:
                            context = dict(context, part=marker)
                        value = {"kind": "marker", "marker": marker}
                        counts["marker_fields"] += 1
                role = "model" if tag == "MODL" and kind != "EYES" else "texture" if tag == "ICON" and kind in {"RACE", "HAIR", "EYES"} else "model_list" if tag == "NIFZ" and kind == "CREA" else "animation_list" if tag == "KFFZ" and kind in {"NPC_", "CREA"} else None
                if role:
                    strings = []
                    if role in {"model", "texture"}:
                        require(data.endswith(b"\0"), "string is not NUL terminated")
                        require(data.find(b"\0", 0, len(data) - 1) < 0, "embedded NUL in string")
                        segments = [(0, len(data) - 1)]
                        if kind != "RACE":
                            singleton = "multiple_actor_model_fields" if role == "model" else "multiple_actor_texture_fields"
                        elif any(v is None for v in context.values()):
                            definition["findings"].append({"field_decoded_offset": at, "code": "race_path_without_part_context"})
                    else:
                        require(not data or data.endswith(b"\0"), "unterminated dependency string frame")
                        segments = frame_offsets(data)
                        singleton = "multiple_actor_model_list_fields" if role == "model_list" else "multiple_actor_animation_list_fields"
                    for pos, end in segments:
                        require(counts["strings"] < 1_000_000 and end - pos <= 32 * MIB - counts["path_bytes"], "dependency string/path byte budget")
                        segment = data[pos:end]
                        counts["strings"] += 1
                        counts["path_bytes"] += len(segment)
                        counts["empty_strings"] += not segment
                        strings.append({"field_byte_offset": pos, "raw": list(segment)})
                    value = {"kind": "paths", "role": role, "context": dict(context) if kind == "RACE" else {"region": None, "sex": None, "part": None}, "strings": strings}
                    counts["path_fields"] += 1
                role = {"PNAM": "head_part", "HNAM": "hair", "ENAM": "eyes"}.get(tag) if kind == "NPC_" else "extra_head_part" if kind == "HDPT" and tag == "HNAM" else {"HNAM": "hair", "ENAM": "eyes"}.get(tag) if kind == "RACE" else None
                if role:
                    require(len(data) % 4 == 0 and (kind == "RACE" or len(data) == 4), "dependency link shape")
                    bindings = []
                    for pos in range(0, len(data), 4):
                        require(counts["bindings"] < 1_000_000, "dependency binding budget")
                        binding = self.binding(entry["source"], struct.unpack_from("<I", data, pos)[0])
                        allowed = None if binding["target"] is None else bytes(binding["target"]["kind"]) == {"head_part": b"HDPT", "extra_head_part": b"HDPT", "hair": b"HAIR", "eyes": b"EYES"}[role]
                        bindings.append({"field_byte_offset": pos, "binding": binding, "schema_kind_allowed": allowed})
                        counts["bindings"] += 1
                    value = {"kind": "links", "role": role, "bindings": bindings}
                    counts["link_fields"] += 1
                    if kind == "NPC_" and role in {"hair", "eyes"}:
                        singleton = f"multiple_actor_{role}_fields"
                if singleton:
                    seen[singleton] += 1
                    if seen[singleton] > 1:
                        definition["findings"].append({"field_decoded_offset": at, "code": singleton})
                definition["fields"].append({"kind": list(tag.encode("latin1")), "decoded_offset": at, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "value": value})
                counts["fields"] += 1
                if kind == "NPC_" and tag == "RNAM":
                    require(len(data) == 4 and counts["bindings"] < 1_000_000, "dependency race link shape/budget")
                    binding = self.binding(entry["source"], struct.unpack("<I", data)[0])
                    definition["race_links"].append({"field_index": field_index, "field_decoded_offset": at, "binding": binding, "schema_kind_allowed": None if binding["target"] is None else bytes(binding["target"]["kind"]) == b"RACE"})
                    counts["bindings"] += 1
                    counts["race_links"] += 1
            counts["source_findings"] += len(definition["findings"])
        return definitions, counts

    def archives_index(self):
        for path in sorted(p for p in self.data.iterdir() if p.suffix.lower() == ".bsa"):
            self.guard()
            source = Source(path)
            self.archives.append(source)
            magic, version, start, flags, folders, files, folder_names, file_names, _ = struct.unpack("<4s8I", source.read(0, 36))
            require(magic == b"BSA\0" and version == 104 and flags & 3 == 3, "independent BSA named v104 admission")
            require(folders <= 1_000_000 and files <= 1_000_000 and folder_names + file_names <= 64 * MIB, "independent BSA metadata budget")
            records, cursor, physical = [], start, []
            for _ in range(folders):
                _, count, offset = struct.unpack("<QII", source.read(cursor, 16))
                require(count <= files, "BSA folder count")
                records.append((count, offset))
                cursor += 16
            require(sum(count for count, _ in records) == files, "BSA file count")
            for count, offset in records:
                require(cursor <= offset <= source.size, "BSA folder lower bound")
                length = source.read(cursor, 1)[0]
                folder = terminated(source.read(cursor + 1, length))
                folder = b"" if folder == b"." else folder
                cursor += length + 1
                for _ in range(count):
                    _, size, position = struct.unpack("<QII", source.read(cursor, 16))
                    require(position <= source.size and (size & 0x3FFFFFFF) <= source.size - position, "BSA member extent")
                    physical.append(folder)
                    cursor += 16
            names = source.read(cursor, file_names)
            at = 0
            for i, folder in enumerate(physical):
                if i % 8192 == 0:
                    self.guard()
                end = names.find(b"\0", at)
                require(end >= at, "BSA name terminator")
                name = names[at:end]
                at = end + 1
                original = folder + b"\\" + name if folder else name
                lookup = normalize(original)
                self.mounts[lookup].append({"container": str(path), "entry_index": i, "original_path": list(original)})
            require(not names[at:].strip(b"\0"), "BSA name padding")

    def manifest(self, root, definitions, graph, remaining):
        require(root in definitions and not definitions[root]["deleted"] and self.winners[root]["kind_name"] in {"NPC_", "CREA"}, "actor manifest root")
        graph_keys = {key_tuple(n["key"]) for n in graph["nodes"]}
        outgoing = collections.defaultdict(list)
        for i, edge in enumerate(graph["edges"]):
            outgoing[key_tuple(edge["source"])].append(i)
        require(remaining["nodes"] > 0, "manifest node budget")
        seen, queue, selected = {root}, collections.deque([root]), []
        while queue:
            for i in outgoing[queue.popleft()]:
                require(len(selected) < remaining["edges"], "manifest edge budget")
                selected.append(i)
                edge = graph["edges"][i]
                target = key_tuple(edge["target"])
                if edge["status"] == "defined" and target in graph_keys and target not in seen:
                    require(len(seen) < remaining["nodes"], "manifest node budget")
                    seen.add(target)
                    queue.append(target)
        closure = {"root": key_json(root), "nodes": [key_json(k) for k in sorted(seen)], "edge_indices": sorted(selected)}
        queue = collections.deque(k for k in sorted(seen) if k in definitions)
        edges, paths = [], []
        counts = {k: 0 for k in ["nodes", "model_source_nodes", "inventory_edges", "model_edges", "field_visits", "paths", "path_bytes", "candidates", "candidate_bytes", "cyclic_components", "cyclic_nodes"]}
        counts["lookup_statuses"] = {}
        while queue:
            self.guard()
            key = queue.popleft()
            definition = definitions[key]
            links = []
            for i, field in enumerate(definition["fields"]):
                require(counts["field_visits"] < remaining["field_visits"], "manifest field visit budget")
                counts["field_visits"] += 1
                value = field["value"]
                if value["kind"] == "links":
                    links.extend((field["decoded_offset"], item["field_byte_offset"], value["role"], item["binding"], item["schema_kind_allowed"]) for item in value["bindings"])
                if value["kind"] != "paths":
                    continue
                for string in value["strings"]:
                    raw = bytes(string["raw"])
                    require(counts["paths"] < remaining["paths"] and len(raw) <= remaining["path_bytes"] - counts["path_bytes"], "manifest path budget")
                    lookup, status, candidates = None, "empty_source_path", []
                    role = value["role"]
                    if raw:
                        if role in {"model_list", "animation_list"}:
                            status = "relative_base_unresolved"
                        elif len(raw) > 4096:
                            status = "lookup_path_too_long"
                        else:
                            try:
                                lookup = normalize(b"meshes/" + raw) if role == "model" else normalize(raw)
                                if role == "texture" and not lookup.startswith(b"textures/"):
                                    lookup = normalize(b"textures/" + lookup)
                                candidates = self.mounts.get(lookup, [])
                                status = "missing_archive_candidate" if not candidates else "one_archive_candidate" if len(candidates) == 1 else "archive_collision"
                            except ValueError:
                                lookup, status = None, "unsafe_asset_path"
                    require(len(candidates) <= remaining["candidates"] - counts["candidates"], "manifest candidate budget")
                    candidate_bytes = sum(len(c["container"].encode("utf-8")) + len(c["original_path"]) for c in candidates)
                    require(candidate_bytes <= remaining["candidate_bytes"] - counts["candidate_bytes"], "manifest candidate byte budget")
                    paths.append({"source": key_json(key), "field_index": i, "field_decoded_offset": field["decoded_offset"], "field_byte_offset": string["field_byte_offset"], "role": role, "context": value["context"], "raw": string["raw"], "asset_path": None if lookup is None else list(lookup), "lookup_status": status, "candidates": candidates})
                    counts["paths"] += 1
                    counts["path_bytes"] += len(raw)
                    counts["candidates"] += len(candidates)
                    counts["candidate_bytes"] += candidate_bytes
                    counts["lookup_statuses"][status] = counts["lookup_statuses"].get(status, 0) + 1
            links.extend((link["field_decoded_offset"], 0, "race", link["binding"], link["schema_kind_allowed"]) for link in definition["race_links"])
            for at, pos, role, binding, allowed in links:
                require(len(edges) + len(selected) < remaining["edges"], "manifest edge budget")
                target = key_tuple(binding["key"])
                if binding["status"] == "defined" and allowed is True and target not in seen:
                    require(target in definitions and not definitions[target]["deleted"], "defined dependency source missing")
                    require(len(seen) < remaining["nodes"], "manifest node budget")
                    seen.add(target)
                    queue.append(target)
                edges.append({"source": key_json(key), "field_decoded_offset": at, "field_byte_offset": pos, "role": role, "binding": binding, "schema_kind_allowed": allowed})
        keys = sorted(seen)
        positions = {key: i for i, key in enumerate(keys)}
        children = [[] for _ in keys]
        for edge in [graph["edges"][i] for i in selected]:
            target = key_tuple(edge["target"])
            if edge["status"] == "defined" and target in positions:
                children[positions[key_tuple(edge["source"])]].append(positions[target])
        for edge in edges:
            target = key_tuple(edge["binding"]["key"])
            if edge["binding"]["status"] == "defined" and target in positions:
                children[positions[key_tuple(edge["source"])]].append(positions[target])
        groups = cycles(children)
        model_nodes = [i for i, key in enumerate(keys) if key in definitions]
        counts.update(nodes=len(keys), model_source_nodes=len(model_nodes), inventory_edges=len(selected), model_edges=len(edges), cyclic_components=len(groups), cyclic_nodes=sum(map(len, groups)))
        for name in remaining:
            remaining[name] -= counts[name] if name != "edges" else len(selected) + len(edges)
        edges.sort(key=lambda e: (key_tuple(e["source"]), e["field_decoded_offset"], e["field_byte_offset"], e["role"]))
        paths.sort(key=lambda p: (key_tuple(p["source"]), p["field_decoded_offset"], p["field_byte_offset"]))
        return {"root": key_json(root), "inventory_closure": closure, "nodes": [key_json(k) for k in keys], "model_source_node_indices": model_nodes, "model_edges": edges, "paths": paths, "cyclic_components": groups, "counts": counts, "scope": "Authored structural dependencies and archive candidates; terminal inventory bodies and NIFZ/KFFZ relative bases remain unresolved; no inheritance, equipment, gender/part choice, playback or retail lookup precedence"}


def normalize(raw):
    require(raw and raw[0] not in b"/\\" and all(b >= 32 and b != 58 for b in raw), "unsafe asset path")
    components = raw.replace(b"\\", b"/").split(b"/")
    require(all(c not in {b"", b".", b".."} for c in components), "invalid asset path component")
    return b"/".join(components).lower()


def cycles(children):
    number, low, active, stack, result = {}, {}, set(), [], []
    for root in range(len(children)):
        if root in number:
            continue
        number[root] = low[root] = len(number)
        active.add(root)
        stack.append(root)
        frames = [[root, 0]]
        while frames:
            node, child = frames[-1]
            if child < len(children[node]):
                target = children[node][child]
                frames[-1][1] += 1
                if target not in number:
                    number[target] = low[target] = len(number)
                    active.add(target)
                    stack.append(target)
                    frames.append([target, 0])
                elif target in active:
                    low[node] = min(low[node], number[target])
            else:
                if low[node] == number[node]:
                    component = []
                    while True:
                        member = stack.pop()
                        active.remove(member)
                        component.append(member)
                        if member == node:
                            break
                    if len(component) > 1 or node in children[node]:
                        result.append(sorted(component))
                frames.pop()
                if frames:
                    parent = frames[-1][0]
                    low[parent] = min(low[parent], low[node])
    return sorted(result)


def render_manifest(reader, root, definitions, manifest, remaining, content_digest):
    """Independent declaration selector; raw ACBS comes from locked plugin bytes.

    Reuse this oracle's original-byte manifest. Never read Rust requests or infer
    an equipped item from the inventory graph. Source declarations are not retail
    actor composition or template inheritance measurements.
    """
    configuration = []
    root_fields = list(fields(reader.payload(root, 64 * MIB)))
    for index, (tag, offset, data) in enumerate(root_fields):
        if tag == "ACBS":
            require(len(data) == 24, "render ACBS source shape")
            configuration.append(dict(inventory_field_index=index,
                field_decoded_offset=offset, flags=struct.unpack_from("<I", data)[0],
                template_flags=struct.unpack_from("<H", data, 22)[0]))
    paths_by_source, edges_by_source = collections.defaultdict(list), collections.defaultdict(list)
    for index, path in enumerate(manifest["paths"]):
        paths_by_source[key_tuple(path["source"])].append(index)
    for index, edge in enumerate(manifest["model_edges"]):
        edges_by_source[key_tuple(edge["source"])].append(index)
    requests, issues, edges = [], [], []
    visits = len(root_fields)
    selected, queue, sex = {root}, collections.deque(), None

    def issue(code, source, edge=None, path=None):
        require(len(issues) < remaining["issues"], "render issue budget")
        issues.append(dict(code=code, source=key_json(source),
            manifest_edge_index=edge, manifest_path_index=path))

    config = configuration[0] if len(configuration) == 1 else None
    if config is None:
        issue("missing_actor_configuration" if not configuration else "ambiguous_actor_configuration", root)
    else:
        if config["template_flags"] & 0x40:
            issue("model_template_selection_unsupported", root)
        else:
            queue.append(root)
        if reader.winners[root]["kind_name"] == "NPC_":
            if config["template_flags"] & 1:
                issue("traits_template_selection_unsupported", root)
            else:
                sex = "female" if config["flags"] & 1 else "male"

    def path_group(value):
        context = value["context"]
        return (value["role"], tuple(context["region"]["kind"]) if context["region"] else None,
            tuple(context["sex"]["kind"]) if context["sex"] else None,
            context["part"]["raw_index"] if context["part"] else None)

    while queue:
        reader.guard()
        key = queue.popleft()
        kind = reader.winners[key]["kind_name"]
        source_paths = paths_by_source[key]
        source_edges = edges_by_source[key]
        source_fields = definitions[key]["fields"]
        visits += len(source_paths) + len(source_fields) + 2 * len(source_edges)
        groups = collections.Counter(path_group(field["value"]) for field in source_fields
            if field["value"]["kind"] == "paths")
        first = len(requests)
        for index in source_paths:
            path = manifest["paths"][index]
            role = None
            if kind in {"NPC_", "CREA"}:
                role = {"model": "actor_model", "animation_list": "animation_list"}.get(path["role"])
                if kind == "CREA" and path["role"] == "model_list":
                    role = "creature_model_list"
            elif kind == "HDPT" and path["role"] == "model":
                role = "head_part"
            elif kind == "HAIR" and path["role"] in {"model", "texture"}:
                role = "hair"
            elif kind == "EYES" and path["role"] == "texture":
                role = "eyes"
            elif kind == "RACE" and path["role"] in {"model", "texture"}:
                region, marker, part = (path["context"][name] for name in ["region", "sex", "part"])
                if None in (region, marker, part):
                    issue("race_path_without_part_context", key, path=index)
                    continue
                if sex is None or bytes(marker["kind"]) != (b"FNAM" if sex == "female" else b"MNAM"):
                    continue
                tag, raw_index = bytes(region["kind"]), part["raw_index"]
                if tag == b"NAM0" and raw_index is not None and raw_index < 8:
                    role = dict(kind="race_head", part_index=raw_index)
                elif tag == b"NAM1" and raw_index is not None and raw_index < 4:
                    role = dict(kind="race_body", part_index=raw_index)
                else:
                    issue("unsupported_race_part_index", key, path=index)
                    continue
            if role is None:
                continue
            require(len(requests) < remaining["requests"], "render request budget")
            requests.append(dict(manifest_path_index=index,
                role=dict(kind=role) if isinstance(role, str) else role,
                ambiguous_source=groups[path_group(path)] > 1))
        visits += len(requests) - first
        expected = "texture" if kind == "EYES" else "model"
        if not any(manifest["paths"][request["manifest_path_index"]]["role"] == expected for request in requests[first:]):
            issue("missing_selected_model_or_texture", key)
        counts = collections.Counter(manifest["model_edges"][index]["role"] for index in source_edges)
        for index in source_edges:
            edge = manifest["model_edges"][index]
            role = edge["role"]
            if not ((kind == "NPC_" and (role in {"head_part", "hair", "eyes"}
                or (role == "race" and sex is not None))) or (kind == "HDPT" and role == "extra_head_part")):
                continue
            edges.append(index)
            if role in {"race", "hair", "eyes"} and counts[role] > 1:
                issue("ambiguous_actor_render_link", key, edge=index)
            elif edge["binding"]["status"] != "defined" or edge["schema_kind_allowed"] is not True:
                issue("unavailable_actor_render_link", key, edge=index)
            else:
                target = key_tuple(edge["binding"]["key"])
                if target not in selected:
                    require(len(selected) < remaining["sources"], "render source budget")
                    selected.add(target)
                    queue.append(target)
        if kind == "NPC_" and sex is not None and counts["race"] == 0:
            issue("missing_actor_race_link", key)
        require(visits <= remaining["visits"], "render visit budget")
    selected_keys = sorted(selected)
    indices = {key: index for index, key in enumerate(selected_keys)}
    children = [[] for _ in selected_keys]
    visits += len(selected_keys) + len(edges)
    require(visits <= remaining["visits"], "render selected graph visit budget")
    for index in sorted(edges):
        edge = manifest["model_edges"][index]
        target = key_tuple(edge["binding"]["key"])
        if edge["binding"]["status"] == "defined" and edge["schema_kind_allowed"] is True and target in indices:
            children[indices[key_tuple(edge["source"])]].append(indices[target])
    selected_cycles = cycles(children)
    for component in selected_cycles:
        for index in component:
            issue("cyclic_selected_render_source", selected_keys[index])
    admitted = bool(requests) and not issues and not selected_cycles and all(
        not request["ambiguous_source"] and manifest["paths"][request["manifest_path_index"]]["lookup_status"] == "one_archive_candidate"
        for request in requests)
    require(len(selected) <= remaining["sources"] and visits <= remaining["visits"], "render source/visit budget")
    result = dict(winning_content_sha256=content_digest, manifest=manifest,
        configuration=config, sex=sex,
        sources=[{name: definitions[key][name] for name in ["key", "source", "header"]} for key in sorted(selected)],
        selected_edge_indices=sorted(edges), selected_source_cycles=selected_cycles,
        requests=requests, selected_requests_admitted=admitted, issues=issues, visits=visits,
        equipment_selection_supported=False,
        scope="Authored actor model/animation and explicit head-part/hair/eye links, sex-bound RACE declarations; no template inheritance, effective equipment, FaceGen composition, relative list base, animation playback or retail precedence")
    for name, count in dict(sources=len(selected), requests=len(requests), issues=len(issues), visits=visits).items():
        remaining[name] -= count
    return result


def template_manifest(reader, root, definitions, graph, closure, remaining, content_digest):
    """Classify original ACBS category words, preserving raw TPLT bindings.

    The existing independently decoded graph supplies structural closure. No
    runtime template value, leveled choice or inheritance rule is manufactured.
    """
    require(root in definitions and not definitions[root]['deleted'] and
        reader.winners[root]['kind_name'] in {'NPC_', 'CREA'}, 'template root actor')
    require(len(closure['nodes']) <= remaining['nodes'] and len(closure['edge_indices']) <= remaining['edges'], 'template closure budget')
    outgoing = collections.defaultdict(list)
    for index, edge in enumerate(graph['edges']):
        outgoing[key_tuple(edge['source'])].append(index)
    selected, queue, sources, links, issues, visits = {root}, collections.deque([root]), {}, [], [], 0

    def issue(code, key, edge=None):
        require(len(issues) < remaining['issues'], 'template issue budget')
        issues.append(dict(code=code, source=key_json(key), graph_edge_index=edge))

    while queue:
        reader.guard()
        key = queue.popleft()
        entry, definition = reader.winners[key], definitions[key]
        raw_fields = list(fields(reader.payload(key, 64 * MIB)))
        source_edges = outgoing[key]
        visits += 2 * len(raw_fields) + len(source_edges)
        require(visits <= remaining['field_visits'], 'template field visit budget')
        configurations, templates = [], []
        for index, (tag, offset, data) in enumerate(raw_fields):
            if tag == 'ACBS':
                require(len(data) == 24, 'template ACBS source shape')
                configurations.append(dict(inventory_field_index=index, field_decoded_offset=offset,
                    flags=struct.unpack_from('<I', data)[0], template_flags=struct.unpack_from('<H', data, 22)[0]))
            elif tag == 'TPLT':
                require(len(data) == 4, 'template TPLT source shape')
                templates.append((index, offset, data))
        config = configurations[0] if len(configurations) == 1 else None
        if config is None:
            issue('missing_actor_configuration' if not configurations else 'ambiguous_actor_configuration', key)
        else:
            if config['template_flags'] & ~0x3ff:
                issue('unknown_template_category_bits', key)
            if config['template_flags'] & 0x3ff and not templates:
                issue('missing_template_link', key)
        by_offset = {graph['edges'][index]['field_decoded_offset']: index
            for index in source_edges if graph['edges'][index]['role'] == 'actor-template'}
        for index, offset, data in templates:
            require(len(links) < remaining['links'], 'template link budget')
            edge_index = by_offset[offset]
            edge = graph['edges'][edge_index]
            binding = reader.binding(entry['source'], struct.unpack('<I', data)[0])
            field = dict(kind=list(b'TPLT'), decoded_offset=offset, bytes=4,
                sha256=hashlib.sha256(data).hexdigest(), value=dict(kind='template', template=binding))
            links.append(dict(graph_edge_index=edge_index, source=key_json(key), inventory_field_index=index,
                field=field, binding=binding, ambiguous_source=len(templates) > 1))
            if len(templates) > 1:
                issue('ambiguous_template_link', key, edge_index)
            elif binding['status'] != 'defined' or edge['schema_kind_allowed'] is not True:
                issue('unavailable_template_link', key, edge_index)
            elif bytes(binding['target']['kind']) in {b'LVLN', b'LVLC'}:
                issue('leveled_template_selection_unsupported', key, edge_index)
            else:
                target = key_tuple(binding['key'])
                if target not in selected:
                    require(len(selected) < remaining['sources'], 'template source budget')
                    selected.add(target)
                    queue.append(target)
        sources[key] = dict(key=definition['key'], source=definition['source'], header=definition['header'], configuration=config)
    require(len(selected) <= remaining['sources'], 'template source budget')
    candidate_sources = [sources[key] for key in sorted(sources)]
    indices = {key: index for index, key in enumerate(sorted(sources))}
    categories = []
    config = sources[root]['configuration']
    for index, category in enumerate(['traits', 'stats', 'factions', 'actor_effects', 'ai_data', 'ai_packages',
        'model_animation', 'base_data', 'inventory', 'script']):
        present = None if config is None else bool(config['template_flags'] & (1 << index))
        selection = dict(status='authored_source', source_index=indices[root]) if present is False else dict(
            status='unsupported', reason='actor_configuration_unavailable' if present is None else 'unverified_template_inheritance')
        categories.append(dict(category=category, mask=1 << index, template_flag_present=present,
            declaration=selection, runtime_value_evaluated=False))
    children = [[] for _ in sources]
    for link in links:
        target = key_tuple(link['binding']['key'])
        if (not link['ambiguous_source'] and link['binding']['status'] == 'defined' and
            graph['edges'][link['graph_edge_index']]['schema_kind_allowed'] is True and target in indices):
            children[indices[key_tuple(link['source'])]].append(indices[target])
    components = cycles(children)
    for component in components:
        for index in component:
            issue('cyclic_template_dependency', key_tuple(candidate_sources[index]['key']))
    for name, count in dict(nodes=len(closure['nodes']), edges=len(closure['edge_indices']), sources=len(sources),
        links=len(links), field_visits=visits, issues=len(issues)).items():
        remaining[name] -= count
    return dict(root=key_json(root), winning_content_sha256=content_digest, structural_closure=closure,
        candidate_sources=candidate_sources, links=links, categories=categories, template_cycles=components,
        issues=issues, field_visits=visits, template_inheritance_supported=False,
        leveled_template_selection_supported=False,
        scope='Pinned editor category masks over exact authored ACBS/TPLT origins and structural actor candidates; no effective inherited fields, leveled template selection, auto-calculated statistics or runtime values')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["data", "load-order", "base-report", "output"]:
        parser.add_argument("--" + name, required=True, type=pathlib.Path)
    parser.add_argument("--root", action="append", default=[])
    parser.add_argument("--root-editor-id", action="append", default=[],
        help="independently resolve an NPC_/CREA winner from its original EDID")
    parser.add_argument("--include-render-dependencies", action="store_true")
    parser.add_argument("--include-template-dependencies", action="store_true")
    parser.add_argument("--equipment-source")
    parser.add_argument("--equipment-role")
    parser.add_argument("--creature-model-directory")
    parser.add_argument("--team-directory", type=pathlib.Path)
    parser.add_argument("--session-id")
    args = parser.parse_args()
    require(len(args.root) + len(args.root_editor_id) <= 64, "actor dependency root budget")
    require(not args.include_render_dependencies or args.root or args.root_editor_id, "render dependencies require explicit roots")
    require(not args.include_template_dependencies or args.root or args.root_editor_id, "template dependencies require explicit roots")
    require(bool(args.equipment_source) == bool(args.equipment_role), "equipment source and role required together")
    require(not args.equipment_source or len(args.root) + len(args.root_editor_id) == 1, "equipment requires one actor root")
    require(not args.creature_model_directory or len(args.root) + len(args.root_editor_id) == 1, "creature directory requires one actor root")

    def guard():
        if args.team_directory is None:
            return
        read = lambda path: json.loads(path.read_text(encoding="utf-8-sig"))
        team = args.team_directory
        control, assignment, lease = read(team.parent / "team/control.json"), read(team / "actors.assignment.json"), read(team / "leases/actors.json")
        require(control["mode"] == "active" and not control["stop_requested"] and assignment["state"] == "active" and assignment["implementation_authorized"] and control["run_id"] == assignment["run_id"] == lease["run_id"] and control["generation"] == assignment["generation"] and lease["session_uuid"] == args.session_id, "actor dependency oracle authorization changed")
        for mailbox in team.glob("*.outbox.jsonl"):
            for line in mailbox.read_text(encoding="utf-8-sig").splitlines():
                if line.strip():
                    row = json.loads(line)
                    require(row.get("run_id") != control["run_id"] or row.get("type") != "stop_requested", "actor dependency oracle observed STOP")
    guard()
    names = json.loads(args.load_order.read_text(encoding="utf-8-sig"))
    require(isinstance(names, list) and 0 < len(names) <= 256, "dependency load order")
    with args.base_report.open("rb") as source:
        raw = source.read(256 * MIB + 1)
    require(len(raw) <= 256 * MIB, "native base report byte budget")
    native = json.loads(raw)
    del raw
    reader = Reader(args.data, names, native, guard)
    try:
        definitions, counts = reader.definitions()
        graph = reader.graph()
        reader.archives_index()
        remaining = dict(nodes=65536, edges=1_000_000, field_visits=2_000_000, paths=100_000, path_bytes=32 * MIB, candidates=100_000, candidate_bytes=32 * MIB)
        roots = []
        for root in args.root:
            origin, local = root.split(":")
            local = int(local, 16)
            require(0 < local <= 0xFFFFFF, "actor dependency root local ID")
            roots.append((plugin_name(origin), local))
        for editor_id in args.root_editor_id:
            require(editor_id.isascii() and 0 < len(editor_id) <= 4096, "actor root EDID")
            matches = []
            for key, definition in definitions.items():
                if definition["deleted"] or reader.winners[key]["kind_name"] not in {"NPC_", "CREA"}:
                    continue
                reader.guard()
                names = [terminated(data) for tag, _, data in fields(reader.payload(key, 64 * MIB)) if tag == "EDID"]
                if any(name.lower() == editor_id.encode("ascii").lower() for name in names):
                    require(len(names) == 1, "actor root has duplicate EDID")
                    matches.append(key)
            require(len(matches) == 1, "actor root EDID is missing or ambiguous")
            roots.extend(matches)
        manifests = [reader.manifest(root, definitions, graph, remaining) for root in roots]
        render_manifests = []
        if args.include_render_dependencies:
            render_remaining = dict(sources=4096, requests=16_384, issues=16_384, visits=2_000_000)
            render_manifests = [render_manifest(reader, root, definitions, manifest, render_remaining,
                native["winning_content_sha256"]) for root, manifest in zip(roots, manifests)]
            native["actor_render_dependencies"] = dict(manifests=render_manifests)
        if args.include_template_dependencies:
            template_remaining = dict(nodes=131072, edges=2_000_000, sources=4096, links=16_384,
                field_visits=2_000_000, issues=16_384)
            native['actor_template_dependencies'] = dict(manifests=[template_manifest(reader, root, definitions,
                graph, manifest['inventory_closure'], template_remaining, native['winning_content_sha256'])
                for root, manifest in zip(roots, manifests)])
        native["actor_dependencies"] = {"counts": counts, "definitions": list(definitions.values()), "inventory_graph": graph, "manifests": [] if args.include_render_dependencies else manifests}
        if args.creature_model_directory:
            from creature_parts import manifest as creature_manifest
            native['actor_creature_parts'] = dict(manifest=creature_manifest(reader, roots[0], definitions, manifests[0],
                args.creature_model_directory.encode('utf-8'), native['winning_content_sha256']))
        if args.equipment_source:
            from equipment import manifest as equipment_manifest
            origin, local = args.equipment_source.split(":")
            equipment_key = (plugin_name(origin), int(local, 16))
            require(0 < equipment_key[1] <= 0xFFFFFF, "equipment key")
            native["actor_equipment_dependencies"] = dict(manifest=equipment_manifest(reader, roots[0], equipment_key, args.equipment_role, native["winning_content_sha256"]))
        guard()
        with args.output.open("x", encoding="utf-8") as output:
            json.dump(native, output, separators=(",", ":"), ensure_ascii=True)
            require(output.tell() <= 256 * MIB, "independent dependency report exceeds 256 MiB")
        print(json.dumps({"records": counts["records"], "fields": counts["fields"], "paths": counts["strings"], "roots": len(manifests), "input_receipts": [{"path": str(s.path), "bytes": s.size, "sha256": s.sha256} for s in reader.sources + reader.archives], "retail_parity_accepted": False}))
    finally:
        reader.close()


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, zlib.error, struct.error) as error:
        raise SystemExit(f"actor-dependencies-oracle: {error}") from error
