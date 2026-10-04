"""Schema3 exact authored/native/original source checks with intended negatives."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

from fixtures import STREAMS, w
from spline_fixtures import VARIANTS, expected, payloads, source
from process_guard import guard, run


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, indent=2); output.write("\n")


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir",type=Path,required=True)
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--oracle",type=Path,required=True)
    parser.add_argument("--retail-inputs",type=Path)
    parser.add_argument("--frozen-schema2-oracle",type=Path)
    args=parser.parse_args(); guard()
    args.output_dir.mkdir(parents=True,exist_ok=False)
    inputs=args.output_dir/"inputs";inputs.mkdir()
    for stream in STREAMS:
        for variant in VARIANTS:
            (inputs/f"stream-{stream}-{variant}.blob").write_bytes(source(stream,variant))
    hashes={p.name:digest(p) for p in inputs.iterdir()}
    binaries=[digest(args.binary),digest(args.oracle)]
    original_hashes={p.name:dict(bytes=p.stat().st_size,sha256=digest(p)) for p in args.retail_inputs.iterdir()} if args.retail_inputs else None
    if original_hashes:write(args.output_dir/"original-inputs-before.json",original_hashes)
    def native(name,directory,schema=3):
        path=args.output_dir/f"{name}-oracle.json"
        command=[str(args.oracle.resolve()),str(directory.resolve())]
        if schema>1:command.append("--include-splines" if schema==3 else "--include-keyframes")
        with path.open("xb") as output:result=run(command,stdout=output,stderr=subprocess.PIPE)
        (args.output_dir/f"{name}-native.stderr.txt").write_bytes(result.stderr)
        document=json.loads(path.read_text(encoding="utf-8"))
        if document.get("oracle_binary_sha256")!=binaries[1]:raise AssertionError("native embedded binary digest differs")
        return result.returncode,path,document
    def rust(name,directory,oracle=None,schema=3):
        path=args.output_dir/f"{name}-rust.json"
        command=[str(args.binary.resolve()),"nif-animation",str(directory.resolve()),"--output",str(path.resolve())]
        if schema>1:command.append("--include-splines" if schema==3 else "--include-keyframes")
        if oracle:command.extend(["--oracle-report",str(oracle.resolve())])
        result=run(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        (args.output_dir/f"{name}-rust.stdout.txt").write_bytes(result.stdout)
        (args.output_dir/f"{name}-rust.stderr.txt").write_bytes(result.stderr)
        document=json.loads(path.read_text()) if path.exists() else None
        return result.returncode,path,document,result.stderr.decode("utf-8",errors="replace")
    code,path,oracle=native("authored",inputs)
    if code:raise AssertionError("native authored source failed; evidence retained")
    code,rust_path,rust_doc,_=rust("authored",inputs,path)
    if code:raise AssertionError("Rust/native authored source failed; evidence retained")
    for row in oracle["files"]:
        variant=row["file"].split("-",2)[2][:-5]
        if [b["data"] for b in row["splines"]]!=expected(variant):
            raise AssertionError(f"native differs from independent authored expectations: {row['file']}")
    if len(oracle["files"])!=len(STREAMS)*len(VARIANTS) or rust_doc["runtime_ready"]:
        raise AssertionError("authored scope/source-only readiness differs")
    def row(document):return next(r for r in document["files"] if r["file"]=="stream-34-combined.blob")
    def block(document,id=0):return row(document)["splines"][id]
    def field(document,id=0):return block(document,id)["data"]
    changes={
        "block_id":lambda d:block(d).update(block=0),
        "block_type":lambda d:block(d).update(block_type="NiBSplineTransformInterpolator"),
        "offset":lambda d:block(d).update(offset=0),
        "bytes":lambda d:block(d).update(bytes=0),
        "hash":lambda d:block(d).update(sha256="0"*64),
        "float_count":lambda d:field(d,2).update(declared_float_count=0),
        "compact_count":lambda d:field(d,2).update(declared_compact_count=0),
        "compact_sign":lambda d:field(d,2)["compact"].__setitem__(0,32768),
        "compact_order":lambda d:field(d,2)["compact"].reverse(),
        "float_zero_bits":lambda d:field(d,2)["float_bits"].__setitem__(0,0),
        "float_order":lambda d:field(d,2)["float_bits"].reverse(),
        "basis_count":lambda d:field(d,1).update(num_control_points=3),
        "source_link":lambda d:field(d).update(spline_data=9),
        "raw_handle":lambda d:field(d).update(scale_handle=4294967295),
        "time_bits":lambda d:field(d).update(start_bits=0),
        "quat_order":lambda d:field(d)["rotation_wxyz_bits"].reverse(),
        "offset_half_range":lambda d:field(d).update(rotation_offset_bits=0),
        "extra_field":lambda d:field(d,2).update(evaluated_pose=True),
        "omission":lambda d:row(d)["splines"].pop(),
        "branch":lambda d:d.update(spline_branch="other"),
        "raw_count_contract":lambda d:d.update(raw_spline_counts_checked=False),
        "schema":lambda d:d.update(schema_version=2),
    }
    negatives=[]
    for name,change in changes.items():
        guard(); altered=copy.deepcopy(oracle);change(altered)
        path=args.output_dir/f"altered-{name}.json";write(path,altered)
        code,_,document,stderr=rust(name,inputs,path)
        reason="provenance contract" if name in ("branch","raw_count_contract","schema") else "oracle spline-source block identity/span/hash or source fields differ"
        if not code or reason not in stderr+json.dumps(document):raise AssertionError(f"unintended/accepted {name}")
        negatives.append(dict(name=name,exit=code,intended_reason=reason))
    interpolator,basis,points=payloads("combined")
    malformed={
        "float_count":({"data":w(4294967295)},"array exceeds"),
        "compact_count":({"data":w(0,4294967295)},"array exceeds"),
        "truncated_compact":({"data":points[:-1]},"array exceeds"),
        "truncated_float":({"data":w(2,0)},"array exceeds"),
        "surplus_points":({"data":points+b"\0"},"surplus bytes"),
        "nan_points":({"data":w(1,0x7fc00001,0)},"nonfinite"),
        "truncated_basis":({"basis":basis[:-1]},"field exceeds"),
        "surplus_basis":({"basis":basis+b"\0"},"surplus bytes"),
        "truncated_interpolator":({"interpolator":interpolator[:-1]},"source span differs"),
        "surplus_interpolator":({"interpolator":interpolator+b"\0"},"source span differs"),
        "nan_offset":({"interpolator":interpolator[:60]+w(0x7fc00001)+interpolator[64:]},"nonfinite"),
        "out_of_range":({"interpolator":interpolator[:8]+w(99)+interpolator[12:]},"link out of range"),
        "wrong_spline_type":({"interpolator":interpolator[:8]+w(9)+interpolator[12:]},"wrong target kind"),
        "wrong_basis_type":({"interpolator":interpolator[:12]+w(10)+interpolator[16:]},"wrong target kind"),
    }
    for name,(replacements,native_reason) in malformed.items():
        guard();directory=args.output_dir/f"bad-{name}";directory.mkdir()
        (directory/"invalid.blob").write_bytes(source(34,"combined",replacements))
        native_code,_,document=native(f"bad-{name}",directory)
        code,_,rust_doc,_=rust(f"bad-{name}",directory)
        native_error=document["files"][0].get("error","");rust_error=rust_doc["files"][0].get("error","")
        reason="unconsumed bytes" if name.startswith("surplus") else "field exceeds" if name.startswith("truncated_interpolator") else "block index out of range" if name=="out_of_range" else native_reason
        if not native_code or not code or native_reason not in native_error or reason not in rust_error:
            raise AssertionError(f"bad {name} accepted/unrelated: {native_error}; {rust_error}")
        for schema in (1,2):
            old_code,_,_,_=rust(f"opaque-{schema}-{name}",directory,schema=schema)
            if old_code:raise AssertionError(f"schema{schema} interpreted malformed spline {name}")
        negatives.append(dict(name=f"malformed-{name}",native_exit=native_code,rust_exit=code,intended_reason=reason))
    regression=[]
    for schema in (1,2):
        code,path,document=native(f"schema{schema}",inputs,schema)
        rust_code,_,_,_=rust(f"schema{schema}",inputs,path,schema)
        if code or rust_code:raise AssertionError(f"schema{schema} regression failed")
        for old,new in zip(document["files"],oracle["files"]):
            if old!={k:v for k,v in new.items() if k not in (("keys","splines") if schema==1 else ("splines",))}:
                raise AssertionError(f"schema{schema} raw projection changed")
        regression.append(dict(schema=schema,files=len(document["files"]),comparison="all_equal"))
    retail=None
    if args.retail_inputs:
        code,path,document=native("retail",args.retail_inputs)
        rust_code,_,rust_doc,_=rust("retail",args.retail_inputs,path)
        if code or rust_code:raise AssertionError("original source comparison failed; evidence retained")
        totals={"float_values":0,"compact_values":0}
        for file in document["files"]:
            for block in file["splines"]:
                data=block["data"]
                if data["kind"]=="control_points":
                    totals["float_values"]+=data["declared_float_count"]
                    totals["compact_values"]+=data["declared_compact_count"]
        retail=dict(files=len(document["files"]),block_counts=rust_doc["block_counts"],values=totals,
                    spline_work=sum(f["splines"]["work_units"] for f in rust_doc["files"]),
                    spline_retained_bytes=sum(f["splines"]["retained_bytes"] for f in rust_doc["files"]),
                    unresolved_dependencies=rust_doc["unresolved_dependencies"],diagnostics=rust_doc["diagnostics"],runtime_ready=False)
        code,path,old=native("retail-schema2",args.retail_inputs,2)
        rust_code,_,_,_=rust("retail-schema2",args.retail_inputs,path,2)
        if code or rust_code:raise AssertionError("original schema2 comparison failed")
        if args.frozen_schema2_oracle:
            frozen=json.loads(args.frozen_schema2_oracle.read_text(encoding="utf-8"))
            if {k:v for k,v in old.items() if k!="oracle_binary_sha256"}!={k:v for k,v in frozen.items() if k!="oracle_binary_sha256"}:
                raise AssertionError("frozen schema2 source/provenance changed")
            code,_,_,_=rust("retail-frozen-schema2",args.retail_inputs,args.frozen_schema2_oracle,2)
            if code:raise AssertionError("frozen schema2 comparison failed")
        retail["schema2_regression"]="all_equal"
    if binaries!=[digest(args.binary),digest(args.oracle)] or hashes!={p.name:digest(p) for p in inputs.iterdir()}:
        raise AssertionError("binary/authored inputs changed during checks")
    if original_hashes:
        after={p.name:dict(bytes=p.stat().st_size,sha256=digest(p)) for p in args.retail_inputs.iterdir()}
        write(args.output_dir/"original-inputs-after.json",after)
        if after!=original_hashes:raise AssertionError("original source inputs changed during checks")
    summary=dict(authored_files=len(hashes),authored_projection="independent_expected_all_equal",native_rust="all_equal",
                 negatives=negatives,schemas=regression,retail=retail,binaries_sha256=binaries,runtime_ready=False)
    write(args.output_dir/"summary.json",summary)
    print(json.dumps(dict(authored_files=len(hashes),negatives=len(negatives),retail=retail)))


if __name__=="__main__":main()
