"""Exact schema2 independent comparisons, intended negatives and schema1 regression."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

from fixtures import STREAMS, w
from keyframe_fixtures import VARIANTS, payload, source
from process_guard import guard, run


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, indent=2)
        output.write("\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--retail-inputs", type=Path)
    parser.add_argument("--frozen-schema1-oracle", type=Path)
    args = parser.parse_args()
    guard()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    inputs = args.output_dir / "inputs"
    inputs.mkdir()
    for stream in STREAMS:
        for variant in VARIANTS:
            (inputs / f"stream-{stream}-{variant}.blob").write_bytes(source(stream, variant))
    original_inputs = {p.name:digest(p) for p in inputs.iterdir()}
    binary_hashes = [digest(args.binary), digest(args.oracle)]

    def native(name, directory, include=True):
        path = args.output_dir / f"{name}-oracle.json"
        command = [str(args.oracle.resolve()), str(directory.resolve())]
        if include:
            command.append("--include-keyframes")
        with path.open("xb") as output:
            result = run(command, stdout=output, stderr=subprocess.PIPE)
        (args.output_dir / f"{name}-native.stderr.txt").write_bytes(result.stderr)
        document = json.loads(path.read_text(encoding="utf-8"))
        if document.get("oracle_binary_sha256") != binary_hashes[1] or digest(args.oracle) != binary_hashes[1]:
            raise AssertionError("native embedded/before/after executable digest differs")
        return result.returncode, path, document

    def rust(name, directory, oracle=None, include=True):
        path = args.output_dir / f"{name}-rust.json"
        command = [str(args.binary.resolve()), "nif-animation", str(directory.resolve()), "--output", str(path.resolve())]
        if include:
            command.append("--include-keyframes")
        if oracle is not None:
            command.extend(["--oracle-report", str(oracle.resolve())])
        result = run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        (args.output_dir / f"{name}-rust.stdout.txt").write_bytes(result.stdout)
        (args.output_dir / f"{name}-rust.stderr.txt").write_bytes(result.stderr)
        return result.returncode, path, result.stderr.decode("utf-8", errors="replace")

    code, oracle_path, oracle = native("authored", inputs)
    if code:
        raise RuntimeError("native authored key decoding failed; preserved report")
    code, rust_path, _ = rust("authored", inputs, oracle_path)
    if code:
        raise RuntimeError("Rust/native authored key comparison failed; preserved report")
    rust_doc = json.loads(rust_path.read_text())
    authored_rust_path = rust_path
    if len(oracle["files"]) != len(STREAMS)*len(VARIANTS) or rust_doc["runtime_ready"]:
        raise AssertionError("authored coverage or source-only readiness differs")

    def data(document, variant="quat2"):
        row = next(r for r in document["files"] if r["file"] == f"stream-34-{variant}.blob")
        return row["keys"][0]["data"]

    def block(document):
        return next(r for r in document["files"] if r["file"] == "stream-34-quat2.blob")["keys"][0]

    changes = {
        "key_block_id":lambda d:block(d).update(block=0),
        "key_block_type":lambda d:block(d).update(block_type="NiKeyframeData"),
        "key_block_offset":lambda d:block(d).update(offset=0),
        "key_block_length":lambda d:block(d).update(bytes=0),
        "key_block_hash":lambda d:block(d).update(sha256="0"*64),
        "key_extra_field":lambda d:data(d).update(invented_default=0),
        "quaternion_tag":lambda d:data(d)["rotation"].update(key_type=1),
        "quaternion_count":lambda d:data(d).update(declared_rotation_keys=99),
        "quaternion_order":lambda d:data(d)["rotation"]["keys"].append(data(d)["rotation"]["keys"].pop(0)),
        "quaternion_signed_zero":lambda d:data(d)["rotation"]["keys"][0]["value_wxyz_bits"].__setitem__(1,0),
        "quaternion_wxyz":lambda d:data(d)["rotation"]["keys"][0]["value_wxyz_bits"].reverse(),
        "quaternion_tbc":lambda d:data(d,"quat3")["rotation"]["keys"][0]["tbc_bits"].__setitem__(0,0),
        "vector_forward":lambda d:data(d)["translations"]["keys"][0]["forward_bits"].__setitem__(0,0),
        "vector_backward":lambda d:data(d)["translations"]["keys"][0]["backward_bits"].__setitem__(0,0),
        "scalar_tbc":lambda d:data(d)["scales"]["keys"][0]["tbc_bits"].__setitem__(0,0),
        "xyz_leading_count":lambda d:data(d,"xyz").update(declared_rotation_keys=2),
        "xyz_axis_order":lambda d:data(d,"xyz")["rotation"]["axes"].reverse(),
        "xyz_absent_tag":lambda d:data(d,"xyz")["rotation"]["axes"][1].update(key_type=0),
        "empty_tag":lambda d:data(d,"empty")["scales"].update(key_type=0),
        "key_omission":lambda d:next(r for r in d["files"] if r["file"]=="stream-34-quat2.blob")["keys"].pop(),
        "raw_count_contract":lambda d:d.update(raw_keyframe_counts_checked=False),
        "keyframe_branch":lambda d:d.update(keyframe_branch="other"),
        "schema":lambda d:d.update(schema_version=1),
    }
    negatives = []
    for name, change in changes.items():
        guard()
        altered = copy.deepcopy(oracle)
        change(altered)
        path = args.output_dir / f"altered-{name}.json"
        write(path,altered)
        code, output, stderr = rust(name,inputs,path)
        if code == 0:
            raise AssertionError(f"accepted altered {name}")
        errors = stderr
        if output.exists():
            errors += json.dumps(json.loads(output.read_text()))
        expected = "provenance contract" if name in ("raw_count_contract","keyframe_branch","schema") else "oracle transform-key block identity/span/hash or source fields differ"
        if expected not in errors:
            raise AssertionError(f"{name} rejected for unrelated reason: {errors[:300]}")
        negatives.append({"name":name,"exit":code,"intended_reason":expected})

    malformed = {
        "quaternion_tag":(w(1,6),"quaternion key tag"),
        "vector_tag":(w(0,1,6),"transform key-group tag"),
        "scalar_tag":(w(0,0,1,6),"transform key-group tag"),
        "xyz_count":(w(2,4,0,0,0,0,0),"XYZ rotation source count other than one"),
        "huge_count":(w(0,0,0xffffffff,1),"array exceeds"),
        "truncated":(payload("quat3")[:-1],"array exceeds"),
        "surplus":(payload("quat2")+b"\0","unconsumed bytes"),
        "nan":(w(1,1,0x7fc00001,0,0,0,0,0,0),"nonfinite"),
    }
    for name,(bad,reason) in malformed.items():
        guard()
        directory=args.output_dir/f"bad-{name}"
        directory.mkdir()
        (directory/"invalid.blob").write_bytes(source(34,"quat2",bad))
        code, _, document=native(f"bad-{name}",directory)
        native_error=document["files"][0].get("error","")
        rust_code, path, _=rust(f"bad-{name}",directory)
        rust_error=json.loads(path.read_text())["files"][0].get("error","")
        native_reason="surplus bytes" if name=="surplus" else reason
        if not code or not rust_code or native_reason not in native_error or reason not in rust_error:
            raise AssertionError(f"bad {name} accepted/unrelated error: {native_error}; {rust_error}")
        # Source schema1 continues to treat the key payload as opaque.
        old_code, _, _ = rust(f"opaque-{name}",directory,include=False)
        if old_code:
            raise AssertionError(f"schema1 started interpreting {name} key data")
        negatives.append({"name":f"malformed-{name}","native_exit":code,"rust_exit":rust_code,"intended_reason":reason})

    retail=None
    if args.retail_inputs:
        code,path,document=native("retail",args.retail_inputs)
        rust_code,rust_path,_=rust("retail",args.retail_inputs,path)
        if code or rust_code:
            raise RuntimeError("retail schema2 source comparison failed; retained reports")
        retail={"files":len(document["files"]),"key_blocks":sum(len(r["keys"]) for r in document["files"]),"native_sha256":digest(path),"rust_sha256":digest(rust_path)}
        if args.frozen_schema1_oracle:
            code,path,current=native("schema1-retail",args.retail_inputs,include=False)
            frozen=json.loads(args.frozen_schema1_oracle.read_text())
            frozen.pop("oracle_binary_sha256",None)
            current.pop("oracle_binary_sha256",None)
            if code or frozen!=current:
                raise AssertionError("schema1 native source/provenance fields changed beyond actual binary hash")
            code,path,_=rust("schema1-retail",args.retail_inputs,args.frozen_schema1_oracle,include=False)
            if code:
                raise AssertionError("new Rust schema1 failed frozen ASSET04 oracle")
    if binary_hashes!=[digest(args.binary),digest(args.oracle)] or original_inputs!={p.name:digest(p) for p in inputs.iterdir()}:
        raise AssertionError("binaries/author inputs changed during comparison")
    write(args.output_dir/"summary.json",{"schema_version":1,"fixtures":len(oracle["files"]),"streams":list(STREAMS),"variants":list(VARIANTS),"negatives":negatives,"retail":retail,"cli_sha256":binary_hashes[0],"oracle_sha256":binary_hashes[1],"authored_native_sha256":digest(oracle_path),"authored_rust_sha256":digest(authored_rust_path),"gameplay_accepted":False})
    print(f"{len(oracle['files'])} authored key sources exact; {len(negatives)} intended negatives; retail={retail}")


if __name__=="__main__":
    main()
