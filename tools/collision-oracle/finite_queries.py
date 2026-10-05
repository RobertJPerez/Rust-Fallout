"""Independent authored finite-query proof; never imports runtime algorithms.

Literal NIF bytes, Fraction geometry/affine inversion, and algebraic comparison
of quadratic roots. Every successful native row must satisfy original caller
interpolation and source membership independently. Output is create-new evidence.
"""
import argparse
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import subprocess
from fractions import Fraction as F


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def seal(path, data):
    with Path(path).open("xb") as file:
        file.write(data)
    os.chmod(path, 0o444)


def json_bytes(value):
    return (json.dumps(value, indent=2, allow_nan=False) + "\n").encode()


def u32(*values):
    return struct.pack("<" + "I" * len(values), *values)


def f32(*values):
    return struct.pack("<" + "f" * len(values), *values)


def word(value):
    return struct.unpack("<I", f32(value))[0]


def fword(value):
    return F(struct.unpack("<f", f32(value))[0])


def hex64(value):
    return struct.pack(">d", value).hex()


IDENTITY = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]]
UNITS = dict(havok_to_source=1., source_to_query=1., transform_tolerance=1e-6)
LIMITS = dict(admission_tests=100000, primitive_tests=100000,
              geometry_tests=1000000, predicate_tests=819200000,
              rows=10000, retained_bytes=16777216)


def body(shape, translation=(0., 0., 0.), quaternion=(0., 0., 0., 1.)):
    value = bytearray(236)
    value[:8] = u32(shape) + bytes([5, 0xe7, 0x34, 0x12])
    value[52:64] = f32(*translation)
    value[68:84] = f32(*quaternion)
    return bytes(value)


def nif(blocks):
    value = b"Gamebryo File Format, Version 20.2.0.7\n"
    value += u32(0x14020007) + bytes([1]) + u32(11, len(blocks), 34)
    value += bytes(3) + struct.pack("<H", len(blocks))
    for name, _ in blocks:
        value += u32(len(name)) + name.encode()
    value += struct.pack("<" + "H" * len(blocks), *range(len(blocks)))
    value += u32(*(len(data) for _, data in blocks)) + u32(0, 0, 0)
    return value + b"".join(data for _, data in blocks) + u32(0)


def geometry(kind, radius=1., endpoints=None):
    base = u32(17) + f32(radius)
    if kind == "sphere":
        return "bhkSphereShape", base, dict(kind=kind, radius_binary32=word(radius))
    if kind == "box":
        return "bhkBoxShape", u32(17) + f32(.25) + bytes(8) + f32(1., 2., 3., 0.), dict(kind=kind, half_extents_binary32=[word(x) for x in [1., 2., 3.]])
    if kind == "capsule":
        a, b = endpoints or ([-1., 0., 0.], [1., 0., 0.])
        data = base + bytes(8) + f32(*a, radius, *b, radius)
        return "bhkCapsuleShape", data, dict(kind=kind, first_binary32=[word(x) for x in a], second_binary32=[word(x) for x in b], radius_binary32=word(radius))
    if kind == "convex_cuboid":
        vertices = [[x, y, z, 0.] for x in [1., 3.] for y in [2., 6.] for z in [-1., 1.]]
        planes = [[-1., 0., 0., 1.], [1., 0., 0., -3.], [0., -1., 0., 2.], [0., 1., 0., -6.], [0., 0., -1., -1.], [0., 0., 1., -1.]]
        data = u32(17) + f32(.25) + u32(0, 0, 0x80000000, 0, 0, 0x80000000, 8)
        data += b"".join(f32(*v) for v in vertices) + u32(6) + b"".join(f32(*p) for p in planes)
        bits64 = lambda v: struct.unpack("<Q", struct.pack("<d", v))[0]
        core = dict(kind=kind, minimum_binary64=[bits64(v) for v in [1., 2., -1.]], maximum_binary64=[bits64(v) for v in [3., 6., 1.]], vertices_binary32=[[word(v) for v in row] for row in vertices], planes_binary32=[[word(v) for v in row] for row in planes])
        return "bhkConvexVerticesShape", data, core
    raise AssertionError(kind)


def triangle_blocks(vertices):
    shape = u32(0, 0) + f32(.125) + u32(0) + f32(1., 1., 1., 0., .125, 1., 1., 1., 0.) + u32(2)
    data = u32(1) + struct.pack("<4H", 0, 1, 2, 0xabcd) + u32(3) + bytes([0])
    data += b"".join(f32(*v) for v in vertices) + struct.pack("<H", 1) + bytes([7, 0x81, 0x34, 0x12]) + u32(3, 42)
    return [("bhkRigidBody", body(1)), ("bhkPackedNiTriStripsShape", shape), ("hkPackedNiTriStripsData", data)]


def subtract(a, b):
    return [x-y for x, y in zip(a, b)]


def dot(a, b):
    return sum(x*y for x, y in zip(a, b))


def cross(a, b):
    return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]


def invert(frame):
    matrix = [[F(x) for x in row[:3]] + [F(i == j) for j in range(3)] for i, row in enumerate(frame)]
    for i in range(3):
        pivot = next(j for j in range(i, 3) if matrix[j][i])
        matrix[i], matrix[pivot] = matrix[pivot], matrix[i]
        coefficient = matrix[i][i]
        matrix[i] = [x/coefficient for x in matrix[i]]
        for j in range(3):
            if i != j:
                coefficient = matrix[j][i]
                matrix[j] = [x-coefficient*y for x, y in zip(matrix[j], matrix[i])]
    return [row[3:] for row in matrix]


def local(frame, point, vector=False):
    input_value = [F(x) for x in point]
    if not vector:
        input_value = subtract(input_value, [F(row[3]) for row in frame])
    return [dot(row, input_value) for row in invert(frame)]


def inside(kind, point, radius=1., vertices=None):
    if kind == "sphere":
        return dot(point, point) <= F(radius)**2
    if kind in ("box", "convex_cuboid"):
        lower, upper = ([-1, -2, -3], [1, 2, 3]) if kind == "box" else ([1, 2, -1], [3, 6, 1])
        return all(a <= x <= b for a, x, b in zip(lower, point, upper))
    if kind == "capsule":
        a, b = [[fword(x) for x in row] for row in (vertices or [[-1., 0., 0.], [1., 0., 0.]])]
        edge = subtract(b, a)
        length = dot(edge, edge)
        parameter = max(F(0), min(F(1), dot(subtract(point, a), edge)/length)) if length else F(0)
        nearest = [x+parameter*d for x, d in zip(a, edge)]
        delta = subtract(point, nearest)
        return dot(delta, delta) <= F(radius)**2
    a, b, c = [[fword(x) for x in row] for row in vertices]
    normal = cross(subtract(b, a), subtract(c, a))
    if not any(normal):
        # The literal degenerate fixture is the closed X-axis segment[-1,1].
        return -1 <= point[0] <= 1 and point[1] == point[2] == 0
    if dot(subtract(point, a), normal) != 0:
        return False
    return all(dot(cross(subtract(y, x), subtract(point, x)), normal) >= 0 for x, y in [(a, b), (b, c), (c, a)])


class Root:
    """Exact algebraic(-b + sign*sqrt(b*b-a*c))/a; no floating sqrt."""
    def __init__(self, a, b, c, sign):
        self.a, self.b, self.discriminant, self.sign = a, b, b*b-a*c, sign
        assert a > 0 and self.discriminant >= 0

    def compare(self, value):
        """Sign of rational value minus this exact root."""
        y = self.a*F(value)+self.b
        if self.sign < 0:
            if y >= 0:
                return 0 if y == self.discriminant == 0 else 1
            return -sign(y*y-self.discriminant)
        if y <= 0:
            return 0 if y == self.discriminant == 0 else -1
        return sign(y*y-self.discriminant)


def sign(value):
    return (value > 0)-(value < 0)


def boundary_check(bounds, boundary):
    a, b = map(F, bounds)
    assert a <= b
    if isinstance(boundary, Root):
        assert boundary.compare(a) <= 0 <= boundary.compare(b), (bounds, "algebraic boundary")
    else:
        assert a <= boundary <= b, (bounds, str(boundary))


def expected_span(kind, origin, direction, maximum, radius=1., literal_span=None):
    maximum = F(maximum)
    if literal_span is not None:
        return literal_span
    if kind == "sphere":
        a, b, c = dot(direction, direction), dot(origin, direction), dot(origin, origin)-F(radius)**2
        if a == 0:
            return (F(0), maximum) if c <= 0 else None
        if b*b-a*c < 0:
            return None
        near, far = Root(a, b, c, -1), Root(a, b, c, 1)
        if near.compare(maximum) < 0 or far.compare(0) > 0:
            return None
        return (F(0) if near.compare(0) >= 0 else near, maximum if far.compare(maximum) <= 0 else far)
    if kind in ("box", "convex_cuboid"):
        minimum, upper = ([-1, -2, -3], [1, 2, 3]) if kind == "box" else ([1, 2, -1], [3, 6, 1])
        entry, exit_value = F(0), maximum
        for x, d, low, high in zip(origin, direction, minimum, upper):
            if d == 0:
                if not low <= x <= high:
                    return None
            else:
                a, b = sorted([(low-x)/d, (high-x)/d])
                entry, exit_value = max(entry, a), min(exit_value, b)
        return (entry, exit_value) if entry <= exit_value else None
    raise AssertionError("A separately derived literal span is required")


class Proof:
    def __init__(self, args):
        self.args, self.root, self.calls, self.sources = args, Path(args.output), [], {}
        self.root.mkdir(parents=True, exist_ok=False)
        for name in ["sources", "requests", "reports", "logs"]:
            (self.root/name).mkdir()
        self.binary_hashes = {args.exe: sha(args.exe), args.old_exe: sha(args.old_exe)}
        seal(self.root/"intake.json", json_bytes(dict(binary_hashes=self.binary_hashes, oracle_sha256=sha(__file__), method="literal authored bytes; exact Fraction original interpolation, affine inverse, core membership, algebraic root comparisons; no producer-generated expectation")))

    def guard(self):
        run = subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(self.root.parent/"session-guard.ps1")], cwd=self.args.worktree, capture_output=True, timeout=15, creationflags=subprocess.CREATE_NO_WINDOW)
        assert run.returncode == 0, run.stderr.decode(errors="replace")
        for path, digest in self.binary_hashes.items():
            assert sha(path) == digest
        for path, digest in self.sources.items():
            assert sha(path) == digest

    def source(self, name, blocks):
        path = self.root/"sources"/(name+".nif")
        data = nif(blocks)
        seal(path, data)
        self.sources[str(path)] = sha(path)
        # Independent header/size table and whole payload concatenation audit.
        cursor = len(b"Gamebryo File Format, Version 20.2.0.7\n")
        assert struct.unpack_from("<I", data, cursor)[0] == 0x14020007
        cursor += 4
        assert data[cursor] == 1
        assert struct.unpack_from("<3I", data, cursor+1) == (11, len(blocks), 34)
        cursor += 16
        assert struct.unpack_from("<H", data, cursor)[0] == len(blocks)
        cursor += 2
        for name, _ in blocks:
            count = struct.unpack_from("<I", data, cursor)[0]
            assert data[cursor+4:cursor+4+count] == name.encode()
            cursor += 4+count
        assert struct.unpack_from("<"+"H"*len(blocks), data, cursor) == tuple(range(len(blocks)))
        cursor += 2*len(blocks)
        assert struct.unpack_from("<"+"I"*len(blocks), data, cursor) == tuple(len(payload) for _, payload in blocks)
        cursor += 4*len(blocks)+12
        for _, payload in blocks:
            assert data[cursor:cursor+len(payload)] == payload
            cursor += len(payload)
        assert data[cursor:] == u32(0)
        return path

    def invoke(self, name, source, request, refusal=None, old=False, audit=True):
        if len(self.calls) % 8 == 0:
            self.guard()
        request_path = self.root/"requests"/(name+".json")
        report_path = self.root/"reports"/(name+".json")
        seal(request_path, json_bytes(request))
        self.sources[str(request_path)] = sha(request_path)
        command = [self.args.old_exe if old else self.args.exe, "--output", str(report_path), "nif-collision", str(source), "--query-request", str(request_path)]
        result = subprocess.run(command, capture_output=True, timeout=15, creationflags=subprocess.CREATE_NO_WINDOW)
        seal(self.root/"logs"/(name+".stdout"), result.stdout)
        seal(self.root/"logs"/(name+".stderr"), result.stderr)
        row = dict(name=name, source=str(source), source_sha256=sha(source), request=str(request_path), request_sha256=sha(request_path), binary_sha256=self.binary_hashes[command[0]], exit_code=result.returncode, expectation=refusal or "complete independent report")
        self.calls.append(row)
        with (self.root/"calls.jsonl").open("ab") as log:
            log.write((json.dumps(row, allow_nan=False)+"\n").encode())
        if refusal:
            error = result.stderr.decode(errors="replace")
            assert result.returncode != 0 and not report_path.exists() and not result.stdout.strip(), (name, result.returncode, error)
            assert refusal in error, (name, error)
            if audit and ("segment_cast" in request or "ray_intervals" in request):
                field = "segment_cast" if "segment_cast" in request else "ray_intervals"
                numbers = self.audit(request, field)
                assert all(word in error for word in numbers), (name, "missing consumed binary64 audit", error)
            return None
        assert result.returncode == 0 and report_path.exists(), (name, result.stderr.decode(errors="replace"))
        os.chmod(report_path, 0o444)
        report = json.loads(report_path.read_bytes())
        assert report["source_sha256"] == sha(source) and report["request_sha256"] == sha(request_path)
        assert report["faithful_ready"] is False
        return report

    @staticmethod
    def audit(request, field):
        query = request[field]
        input_value = query["segment"] if field == "segment_cast" else query["ray"]
        values = list(input_value.values())
        words = [hex64(x) for value in values for x in (value if isinstance(value, list) else [value])]
        words += [hex64(x) for x in request["units"].values()]
        words += [hex64(x) for row in request["attachment_rows"] for x in row]
        return words

    def checked(self, name, source, request, kinds, frames=None, radii=None, vertices=None, literal_spans=None, core_words=None, shape_blocks=None):
        report = self.invoke(name, source, request)
        field = "segment_cast" if "segment_cast" in request else "ray_intervals"
        finite = report[field]
        assert report["units"] == request["units"]
        audit_words = self.audit(request, field)
        values = finite["numeric_input"]
        actual = [x for v in values.values() for x in (v if isinstance(v, list) else [v])]
        actual += finite["environment_numeric_input"]["units_binary64_hex"]
        actual += [x for row in finite["environment_numeric_input"]["attachment_binary64_hex"] for x in row]
        assert actual == audit_words, (name, "consumed binary64 words")
        assert finite["limits"] == request[field]["limits"]
        query = request[field]["segment" if field == "segment_cast" else "ray"]
        origin = [F(x) for x in query["start" if field == "segment_cast" else "origin"]]
        direction = subtract([F(x) for x in query["end"]], origin) if field == "segment_cast" else [F(x) for x in query["direction"]]
        maximum = F(1) if field == "segment_cast" else F(query["max_distance"])
        expected = []
        for i, kind in enumerate(kinds):
            frame = (frames or [request["attachment_rows"]]*len(kinds))[i]
            radius = (radii or [1.]*len(kinds))[i]
            span = expected_span(kind, local(frame, origin), local(frame, direction, True), maximum, radius, (literal_spans or [None]*len(kinds))[i])
            if span is not None:
                expected.append((request["body_blocks"][i], kind, frame, radius, span))
        rows = finite["query"]["results"]
        assert [row["provenance"]["source"]["body_block"] for row in rows] == [x[0] for x in expected], (name, "complete source-qualified rows")
        assert report["primitive_count"] == len(kinds)
        work = finite["query"]["work"]
        assert work["admission_tests"] == work["primitive_tests"] == len(kinds)
        assert work["predicate_tests"] == len(kinds)*8192 and work["geometry_tests"] == kinds.count("triangle")
        assert work["rows"] == len(rows) and 0 <= work["retained_bytes"] <= request[field]["limits"]["retained_bytes"]
        for row, (body_block, kind, frame, radius, span) in zip(rows, expected):
            hit = row["provenance"]
            source_id = hit["source"]
            assert source_id["reference"] == 1 and source_id["source_sha256"] == list(bytes.fromhex(sha(source)))
            assert source_id["body_block"] == body_block and source_id["occurrence"] == 0
            assert source_id["shape_block"] == (shape_blocks or [len(kinds)]*len(kinds))[kinds.index(kind)]
            assert source_id["triangle"] == (0 if kind == "triangle" else None)
            assert hit["body_filter"] == dict(layer=5, flags_and_parts=0xe7, group=0x1234)
            assert hit["material"] == (42 if kind == "triangle" else 17)
            assert hit["welding"] == (0xabcd if kind == "triangle" else None)
            assert hit["shape_filter"] == (dict(layer=7, flags_and_parts=0x81, group=0x1234) if kind == "triangle" else None)
            assert F(hit["authored_shell_radius"]) == F(.125 if kind == "triangle" else .25 if kind in ("box", "convex_cuboid") else 0)
            parameter = F(row["parameter" if field == "segment_cast" else "witness_parameter"])
            assert 0 <= parameter <= maximum
            position = [F(x) for x in hit["position"]]
            assert position == [x+d*parameter for x, d in zip(origin, direction)], (name, "point outside ORIGINAL input line")
            assert inside(kind, local(frame, position), radius, vertices), (name, "point outside authored core")
            boundary_check(row["entry_parameter_bounds"], span[0])
            boundary_check(row["exit_parameter_bounds"], span[1])
            assert all(0 <= F(x) <= maximum for key in ["entry_parameter_bounds", "exit_parameter_bounds"] for x in row[key])
            if field == "ray_intervals":
                assert row["initial_containment"] == inside(kind, local(frame, origin), radius, vertices)
                assert F(hit["distance"]) == parameter
            else:
                low, high = map(F, row["distance_bounds"])
                distance_squared = dot(direction, direction)*parameter**2
                assert 0 <= low <= high and low*low <= distance_squared <= high*high
                assert low <= F(hit["distance"]) <= high
            if core_words is not None:
                assert row["source_core"] == core_words
        return report


def request(field, origin, end_or_direction, maximum=10., bodies=(0,), attachment=None, units=None):
    numbers = dict(start=origin, end=end_or_direction) if field == "segment_cast" else dict(origin=origin, direction=end_or_direction, max_distance=maximum)
    return dict(reference=1, body_blocks=list(bodies), attachment_rows=attachment or copy.deepcopy(IDENTITY), units=units or copy.deepcopy(UNITS), **{field: dict(**{"segment" if field == "segment_cast" else "ray": numbers}, limits=copy.deepcopy(LIMITS), output_bytes=67108864)})


def run(proof):
    catalog = {}
    for kind in ["sphere", "box", "convex_cuboid", "capsule"]:
        name, payload, core = geometry(kind)
        catalog[kind] = (proof.source(kind, [("bhkRigidBody", body(1)), (name, payload)]), core)
    sphere, sphere_core = catalog["sphere"]
    for label, start, end in [
        ("axis", [-3., 0., 0.], [1., 0., 0.]),
        ("reverse", [1., 0., 0.], [-3., 0., 0.]),
        ("endpoint", [-2., 0., 0.], [-1., 0., 0.]),
        ("just-before", [-2., 0., 0.], [-math.nextafter(1., math.inf), 0., 0.]),
        ("near-miss", [-2., math.nextafter(1., math.inf), 0.], [2., math.nextafter(1., math.inf), 0.]),
        ("skew", [-3., .5, 0.], [3., .5, 0.]),
        ("point-inside", [-0., 0., 0.], [-0., 0., 0.]),
        ("point-outside", [2., 0., 0.], [2., 0., 0.]),
        ("lost-delta-inside", [2.**-60, 0., 0.], [1., 0., 0.]),
        ("minsub-inside", [0., 0., 0.], [math.ulp(0.), 0., 0.]),
        ("distant-axis", [-1e20, 0., 0.], [1e20, 0., 0.]),
    ]:
        proof.checked("segment-"+label, sphere, request("segment_cast", start, end), ["sphere"], core_words=sphere_core)
    for label, origin, direction, maximum in [
        ("axis", [-3., 0., 0.], [1., 0., 0.], 10.),
        ("inside", [0., 0., 0.], [1., 0., 0.], .5),
        ("surface-in", [-1., 0., 0.], [1., 0., 0.], 10.),
        ("surface-out", [-1., 0., 0.], [-1., 0., 0.], 10.),
        ("tangent", [-2., 1., 0.], [1., 0., 0.], 10.),
        ("zero-inside", [0., 0., 0.], [1., 0., 0.], 0.),
        ("zero-outside", [2., 0., 0.], [1., 0., 0.], 0.),
        ("skew", [-3., .5, 0.], [1., 0., 0.], 10.),
        ("distant-miss", [1e20, 2., 0.], [-1., 0., 0.], 1e20),
        ("distant-axis", [1e20, 0., 0.], [-1., 0., 0.], 1e20),
    ]:
        proof.checked("interval-"+label, sphere, request("ray_intervals", origin, direction, maximum), ["sphere"], core_words=sphere_core)
    for kind, start, end in [("box", [-3., 0., 0.], [1., 0., 0.]), ("convex_cuboid", [0., 4., 0.], [2., 4., 0.])]:
        source, core = catalog[kind]
        proof.checked("segment-"+kind, source, request("segment_cast", start, end), [kind], core_words=core)
        proof.checked("interval-"+kind, source, request("ray_intervals", start, [1., 0., 0.]), [kind], core_words=core)
    capsule, capsule_core = catalog["capsule"]
    proof.checked("segment-capsule", capsule, request("segment_cast", [-3., 0., 0.], [1., 0., 0.]), ["capsule"], literal_spans=[(F(1, 4), F(1))], core_words=capsule_core)
    proof.invoke("interval-capsule-refusal", capsule, request("ray_intervals", [-3., 0., 0.], [1., 0., 0.]), "solid intervals support")
    vertices = [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]]
    triangle = proof.source("triangle", triangle_blocks(vertices))
    triangle_core = dict(kind="triangle", vertices_binary32=[[word(v) for v in row] for row in vertices])
    for label, start, end, span in [
        ("cross", [.5, .5, -1.], [.5, .5, 1.], (F(1, 2), F(1, 2))),
        ("coplanar", [-1., .5, 0.], [2., .5, 0.], (F(1, 3), F(5, 6))),
        ("point", [0., 0., 0.], [0., 0., 0.], (F(0), F(1))),
    ]:
        # The existing leaf identity points at authored packed DATA block2;
        # wrapper block1 supplies scale/shell, not the triangle vertex table.
        proof.checked("segment-triangle-"+label, triangle, request("segment_cast", start, end), ["triangle"], vertices=vertices, literal_spans=[span], core_words=triangle_core, shape_blocks=[2])
    proof.invoke("interval-triangle-refusal", triangle, request("ray_intervals", [3., 3., -1.], [0., 0., 1.]), "solid intervals support")
    for label, endpoints, start, end, span in [
        ("skew", [[0., 0., 0.], [4., 4., 0.]], [2., 2., -3.], [2., 2., 1.], (F(1, 2), F(1))),
        ("collapsed", [[0., 0., 0.], [0., 0., 0.]], [-3., 0., 0.], [1., 0., 0.], (F(1, 2), F(1))),
    ]:
        name, payload, core = geometry("capsule", endpoints=endpoints)
        source = proof.source("capsule-"+label, [("bhkRigidBody", body(1)), (name, payload)])
        proof.checked("segment-capsule-"+label, source, request("segment_cast", start, end), ["capsule"], vertices=endpoints, literal_spans=[span], core_words=core)
    line_vertices = [[-1., 0., 0.], [1., 0., 0.], [0., 0., 0.]]
    degenerate = proof.source("degenerate-triangle", triangle_blocks(line_vertices))
    proof.checked("segment-degenerate-triangle", degenerate, request("segment_cast", [-2., 0., 0.], [2., 0., 0.]), ["triangle"], vertices=line_vertices, literal_spans=[(F(1, 4), F(3, 4))], core_words=dict(kind="triangle", vertices_binary32=[[word(v) for v in row] for row in line_vertices]), shape_blocks=[2])
    tight_triangle = request("segment_cast", [.5, .5, -1.], [.5, .5, 1.])
    tight_triangle["segment_cast"]["limits"].update(admission_tests=1,primitive_tests=1,geometry_tests=1,predicate_tests=8192,rows=1)
    proof.checked("segment-triangle-exact-geometry", triangle, tight_triangle, ["triangle"], vertices=vertices, literal_spans=[(F(1, 2), F(1, 2))], core_words=triangle_core, shape_blocks=[2])
    for separation, label in [(1., "overlapping"), (4., "disjoint")]:
        name, payload, _ = geometry("sphere")
        cohort = proof.source(label+"-solids", [("bhkRigidBody", body(2)), ("bhkRigidBodyT", body(2, translation=(separation, 0., 0.))), (name, payload)])
        frame = copy.deepcopy(IDENTITY)
        frame[0][3] = separation
        for field in ["segment_cast", "ray_intervals"]:
            # Delta8 gives independently representable points in both spans.
            value = request(field, [-3., 0., 0.], [5., 0., 0.] if field == "segment_cast" else [1., 0., 0.], bodies=(0, 1))
            proof.checked(field+"-"+label, cohort, value, ["sphere", "sphere"], frames=[IDENTITY,frame], core_words=sphere_core)
        if separation == 1.:
            # With delta10, the fixed root/midpoint candidates in the first
            # source span do not certify original binary64 interpolation.
            # The conservative complete refusal remains intentional evidence.
            proof.invoke("segment-unrepresentable-fixed-candidates", cohort, request("segment_cast", [-3., 0., 0.], [7., 0., 0.], bodies=(0, 1)), "witness is numerically uncertain")
    name, payload, point_core = geometry("sphere", 0.)
    point = proof.source("point-sphere", [("bhkRigidBody", body(1)), (name, payload)])
    proof.checked("interval-minsub-point-miss", point, request("ray_intervals", [math.ulp(0.), 0., 0.], [1., 0., 0.], 0.), ["sphere"], radii=[0.], core_words=point_core)
    proof.checked("segment-zero-point-inside", point, request("segment_cast", [0., 0., 0.], [0., 0., 0.]), ["sphere"], radii=[0.], core_words=point_core)
    proof.invoke("segment-minsub-point-numeric-refusal", point, request("segment_cast", [0., 0., 0.], [math.ulp(0.), 0., 0.]), "numerically uncertain")
    for label, origin, direction, maximum in [
        ("original-nonunit-skew", [-1.2, -1.6, 0.], [.6, .8, 0.], 4.),
        ("distant-original-skew-miss", [1e20, 0., 0.], [-1., 2.**-40, 0.], 1e20),
    ]:
        proof.checked(label, sphere, request("ray_intervals", origin, direction, maximum), ["sphere"], core_words=sphere_core)
    for exponent in [-149, -40, 40, 80]:
        radius = 2.**exponent
        name, payload, core = geometry("sphere", radius)
        source = proof.source("sphere-radius-"+str(exponent), [("bhkRigidBody", body(1)), (name, payload)])
        for field in ["segment_cast", "ray_intervals"]:
            request_value = request(field, [-3*radius, 0., 0.], ([radius, 0., 0.] if field == "segment_cast" else [1., 0., 0.]), 10*radius)
            proof.checked(field+"-radius-"+str(exponent), source, request_value, ["sphere"], radii=[radius], core_words=core)
    name, sphere_payload, _ = geometry("sphere")
    cap_name, cap_payload, _ = geometry("capsule")
    far_capsule = proof.source("late-far-capsule", [("bhkRigidBody", body(2)), ("bhkRigidBodyT", body(3, translation=(1e20, 0., 0.))), (name, sphere_payload), (cap_name, cap_payload)])
    proof.invoke("interval-late-far-capsule", far_capsule, request("ray_intervals", [-3., 0., 0.], [1., 0., 0.], bodies=(0, 1)), "solid intervals support")
    transform = u32(4, 0)+f32(0.)+bytes(8)
    transform += b"".join(f32(*column) for column in [[3., 0., 0., 0.], [0., 3., 0., 0.], [0., 0., 3., 0.], [1e20, 0., 0., 1.]])
    far_frame = proof.source("late-far-nonpow2-frame", [("bhkRigidBody", body(2)), ("bhkRigidBody", body(3)), (name, sphere_payload), ("bhkTransformShape", transform), (name, sphere_payload)])
    proof.invoke("interval-late-far-nonpow2-frame", far_frame, request("ray_intervals", [-3., 0., 0.], [1., 0., 0.], bodies=(0, 1)), "signed-axis power-of-two")
    explicit_units = dict(havok_to_source=2., source_to_query=.5, transform_tolerance=1e-6)
    proof.checked("segment-explicit-units", sphere, request("segment_cast", [-3., 0., 0.], [1., 0., 0.], units=explicit_units), ["sphere"], frames=[IDENTITY], core_words=sphere_core)
    proof.checked("interval-explicit-units", sphere, request("ray_intervals", [-3., 0., 0.], [1., 0., 0.], units=explicit_units), ["sphere"], frames=[IDENTITY], core_words=sphere_core)
    for axis in range(3):
        for exponent in [-20, 0, 20]:
            scale = 2.**exponent
            attachment = [[0., 0., 0., 0.] for _ in range(3)]
            for i in range(3):
                attachment[i][(i+axis)%3] = -scale if i == axis else scale
            attachment[axis][3] = 8*scale
            start = [row[3] for row in attachment]
            start[axis] -= 3*scale
            end = [row[3] for row in attachment]
            end[axis] += scale
            direction = [0., 0., 0.]
            direction[axis] = 1.
            proof.checked(f"segment-frame-{axis}-{exponent}", sphere, request("segment_cast", start, end, attachment=attachment), ["sphere"], core_words=sphere_core)
            proof.checked(f"interval-frame-{axis}-{exponent}", sphere, request("ray_intervals", start, direction, 10*scale, attachment=attachment), ["sphere"], core_words=sphere_core)
    name, payload, _ = geometry("sphere")
    generic = proof.source("generic-pose", [("bhkRigidBodyT", body(1, quaternion=(0., 0., .6, .8))), (name, payload)])
    z, w = fword(.6), fword(.8)
    frame = [[1-2*z*z, -2*z*w, F(0), F(0)], [2*z*w, 1-2*z*z, F(0), F(0)], [F(0), F(0), F(1), F(0)]]
    proof.checked("segment-generic-pose", generic, request("segment_cast", [-3., 0., 0.], [3., 0., 0.]), ["sphere"], frames=[frame], core_words=sphere_core)
    proof.invoke("interval-generic-pose-refusal", generic, request("ray_intervals", [-3., 0., 0.], [1., 0., 0.]), "signed-axis power-of-two")
    lost = proof.source("lost-source-pose", [("bhkRigidBody", body(2)), ("bhkRigidBodyT", body(2, translation=(2.**-149, 0., 0.))), (name, payload)])
    attachment = copy.deepcopy(IDENTITY)
    attachment[0][3] = 1.
    for field in ["segment_cast", "ray_intervals"]:
        end = [1., 0., 0.]
        proof.invoke(field+"-late-lost-pose", lost, request(field, [-3., 0., 0.], end, bodies=(0, 1), attachment=attachment), "exact authored transform arithmetic")
    late = triangle_blocks([[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]])
    late += [("bhkRigidBody", body(4)), (name, u32(17)+f32(4.))]
    late_source = proof.source("late-unrepresentable-triangle", late)
    proof.invoke("segment-late-unrepresentable-triangle", late_source, request("segment_cast", [.25, .25, -1.], [.25, .25, 2.], bodies=(3, 0)), "witness is numerically uncertain")
    tied = proof.source("tied-source", [("bhkRigidBody", body(2)), ("bhkRigidBody", body(2)), (name, payload)])
    for field in ["segment_cast", "ray_intervals"]:
        initial = request(field, [-3., 0., 0.], [1., 0., 0.], bodies=(0, 1))
        report = proof.checked(field+"-tie", tied, initial, ["sphere", "sphere"], core_words=sphere_core)
        work = report[field]["query"]["work"]
        exact = copy.deepcopy(initial)
        exact[field]["limits"] = dict(work)
        proof.checked(field+"-exact-work", tied, exact, ["sphere", "sphere"], core_words=sphere_core)
        for dimension in ["admission_tests", "primitive_tests", "predicate_tests", "rows", "retained_bytes"]:
            under = copy.deepcopy(exact)
            under[field]["limits"][dimension] -= 1
            proof.invoke(field+"-one-under-"+dimension, tied, under, "budget")
        # Pretty report length changes only with output_bytes digit count;
        # the request SHA has fixed64 width. Solve that small fixed point.
        original_length = (proof.root/"reports"/(field+"-tie.json")).stat().st_size
        cap = original_length
        while True:
            next_cap = original_length-8+len(str(cap))
            if next_cap == cap:
                break
            cap = next_cap
        output_exact = copy.deepcopy(initial)
        output_exact[field]["output_bytes"] = cap
        proof.checked(field+"-exact-output", tied, output_exact, ["sphere", "sphere"], core_words=sphere_core)
        assert (proof.root/"reports"/(field+"-exact-output.json")).stat().st_size == cap
        output_exact[field]["output_bytes"] -= 1
        proof.invoke(field+"-one-under-output", tied, output_exact, "output budget exceeded")
    geometry_under = request("segment_cast", [.5, .5, -1.], [.5, .5, 1.])
    geometry_under["segment_cast"]["limits"]["geometry_tests"] = 0
    proof.invoke("segment-one-under-geometry", triangle, geometry_under, "finite geometry tests")
    for field in ["segment_cast", "ray_intervals"]:
        huge = request(field, [-1e51, 0., 0.], [1., 0., 0.])
        proof.invoke(field+"-overflow-domain", sphere, huge, "finite")
        ceiling = request(field, [-3., 0., 0.], [1., 0., 0.])
        ceiling[field]["limits"]["rows"] += 1
        proof.invoke(field+"-ceiling-refusal", sphere, ceiling, "only reduce ceilings")
    mixed = request("segment_cast", [-3., 0., 0.], [1., 0., 0.])
    mixed["ray_intervals"] = request("ray_intervals", [-3., 0., 0.], [1., 0., 0.])["ray_intervals"]
    proof.invoke("mixed-forms-refusal", sphere, mixed, "exactly one finite query form", audit=False)
    baseline_pairs = []
    for kind in ["sphere", "box", "convex_cuboid", "capsule"]:
        source, _ = catalog[kind]
        origin = [-3., 0., 0.] if kind != "convex_cuboid" else [0., 4., 0.]
        legacy = dict(reference=1, body_blocks=[0], attachment_rows=copy.deepcopy(IDENTITY), units=copy.deepcopy(UNITS), ray=dict(origin=origin, direction=[1., 0., 0.], max_distance=10.), overlap=dict(center=([0., 0., 0.] if kind != "convex_cuboid" else [2., 4., 0.]), radius=.25))
        proof.invoke("legacy-new-"+kind, source, legacy)
        proof.invoke("legacy-old-"+kind, source, legacy, old=True)
        # Request bytes are equal apart from path; same whole request SHA.
        a, b = [proof.root/"reports"/(prefix+kind+".json") for prefix in ["legacy-new-", "legacy-old-"]]
        assert a.read_bytes() == b.read_bytes(), kind
        baseline_pairs.append(dict(kind=kind, sha256=sha(a)))
    proof.guard()
    seal(proof.root/"result.json", json_bytes(dict(passed=True, calls=len(proof.calls), reports=sum(x["exit_code"] == 0 for x in proof.calls), refusals=sum(x["exit_code"] != 0 for x in proof.calls), source_and_request_hashes=proof.sources, baseline_pairs=baseline_pairs, binary_hashes=proof.binary_hashes, scope="finite authored engineering geometry; every native row independently checked; faithful_ready=false; no gameplay/original measurement")))
    print(json.dumps(dict(passed=True, calls=len(proof.calls), sources=len(catalog), legacy_byte_pairs=len(baseline_pairs))))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True)
    parser.add_argument("--old-exe", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--worktree", required=True)
    args = parser.parse_args()
    proof = Proof(args)
    try:
        run(proof)
    except BaseException as error:
        seal(proof.root/"failure.json", json_bytes(dict(passed=False, calls=len(proof.calls), error=repr(error))))
        raise
