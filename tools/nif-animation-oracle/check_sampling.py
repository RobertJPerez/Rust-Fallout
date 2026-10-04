"""Independent exact-rational math checks; no original animation acceptance."""
import argparse
import copy
from fractions import Fraction as F
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess

from sampling_fixtures import CASES,REFUSALS,source
from process_guard import guard,run

RELATIVE_BOUND=F(1,1<<49)  # Eight binary64 epsilons, conditioned on both endpoints.
ABSOLUTE_BOUND=F(1,1<<1074)  # One least binary64 subnormal.
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def write(path,value):
    with path.open("x",encoding="utf-8") as output:json.dump(value,output,indent=2);output.write("\n")
def f32(word):return struct.unpack("<f",struct.pack("<I",word))[0]
def f64(word):return struct.unpack("<d",struct.pack("<Q",word))[0]
def bits64(value):return struct.unpack("<Q",struct.pack("<d",value))[0]
def rational32(word):
    sign=-1 if word>>31 else 1;exponent=(word>>23)&255;mantissa=word&0x7fffff
    assert exponent!=255
    numerator=sign*(mantissa+(1<<23) if exponent else mantissa)
    power=exponent-150 if exponent else -149
    return F(numerator*(1<<power)) if power>=0 else F(numerator,1<<(-power))
def source_check(row,native):
    if row["sha256"]!=native["sha256"] or row["decoded_bytes"]!=native["decoded_bytes"] or row["keys"]["blocks"]!=native["keys"]:
        raise AssertionError("source identity/ordered key projection differs from independent native source")
def reference(group,time):
    keys=group["keys"]
    if not keys:return None
    t=F.from_float(time);times=[rational32(k["time_bits"]) for k in keys]
    if any(a>=b for a,b in zip(times,times[1:])) or t<times[0] or t>times[-1]:raise AssertionError("reference domain is ambiguous/out of range")
    exact=next((i for i,value in enumerate(times) if value==t),None)
    if exact is not None:left=right=exact
    else:
        # Independent linear interval walk, not the production binary search.
        right=next(i for i,value in enumerate(times) if value>t);left=right-1
    alpha=F(0) if left==right else (t-times[left])/(times[right]-times[left])
    a=keys[left]["value_bits"];b=keys[right]["value_bits"]
    held=left==right or group["key_type"]==5
    values=[rational32(x) if held else rational32(x)+(rational32(y)-rational32(x))*alpha for x,y in zip(a,b)]
    return dict(pair=[left,right],alpha=alpha,values=values,a=a,b=b,source_bits=a if held else None)
def verify(row,native,block_id,channel,time):
    source_check(row,native)
    block=next(b for b in native["keys"] if b["block"]==block_id)
    diagnostic=row["engineering_sample"]
    expected=dict(source_block=block_id,source_block_type=block["block_type"],source_offset=block["offset"],source_bytes=block["bytes"],source_sha256=block["sha256"],requested_time_f64_bits=bits64(time))
    if any(diagnostic[k]!=v for k,v in expected.items()):raise AssertionError("sampling source/request identity differs")
    if diagnostic["contract"]!="engineering-linear-constant-components-v1" or diagnostic["runtime_ready"] is not False:
        raise AssertionError("engineering sampling contract differs")
    evaluation=diagnostic["evaluation"]
    if evaluation["channel"]!=channel:raise AssertionError("requested component differs")
    group=block["data"]["translations" if channel=="translation" else "scales"]
    expected=reference(group,time);sample=evaluation["sample"]
    if expected is None:
        if sample is not None:raise AssertionError("absent source component acquired a value")
        return dict(values=0,max_conditioned_error=0.,max_absolute_error=0.)
    if sample["source_key_indices"]!=expected["pair"] or sample["source_value_bits"]!=expected["source_bits"]:
        raise AssertionError("source bracket or endpoint/held bits differ")
    if sample["interpolation"]!=("linear" if group["key_type"]==1 else "constant"):
        raise AssertionError("interpolation contract differs")
    if not math.isfinite(f64(sample["alpha_f64_bits"])) or abs(F.from_float(f64(sample["alpha_f64_bits"]))-expected["alpha"])>RELATIVE_BOUND+ABSOLUTE_BOUND:
        raise AssertionError("alpha differs from independent rational reference")
    if len(sample["evaluated_f64_bits"])!=len(expected["values"]):raise AssertionError("evaluated component cardinality differs")
    maximum=F(0);absolute=F(0)
    for actual,ideal,a,b in zip(sample["evaluated_f64_bits"],expected["values"],expected["a"],expected["b"]):
        if not math.isfinite(f64(actual)):raise AssertionError("nonfinite evaluated value")
        if expected["source_bits"] is not None and actual!=bits64(f32(a)):
            raise AssertionError("endpoint/held binary64 value differs from source promotion")
        error=abs(F.from_float(f64(actual))-ideal)
        scale=abs(rational32(a))+abs(rational32(b))+abs(ideal)
        if error>RELATIVE_BOUND*scale+ABSOLUTE_BOUND:raise AssertionError("evaluated value exceeds rational-reference tolerance")
        maximum=max(maximum,error/scale if scale else F(0));absolute=max(absolute,error)
    return dict(values=len(expected["values"]),max_conditioned_error=float(maximum),max_absolute_error=float(absolute))

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir",type=Path,required=True)
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--oracle",type=Path,required=True)
    parser.add_argument("--original-inputs",type=Path,required=True)
    parser.add_argument("--original-native-report",type=Path,required=True)
    args=parser.parse_args();guard();args.output_dir.mkdir(parents=True,exist_ok=False)
    inputs=args.output_dir/"inputs";inputs.mkdir()
    for name,case in (CASES|REFUSALS).items():(inputs/f"{name}.blob").write_bytes(source(case))
    binary_hashes=[digest(args.binary),digest(args.oracle)]
    input_hashes={p.name:digest(p) for p in inputs.iterdir()}
    originals={p.name:digest(p) for p in args.original_inputs.iterdir()}
    native_path=args.output_dir/"authored-native.json"
    with native_path.open("xb") as output:
        result=run([str(args.oracle.resolve()),str(inputs.resolve()),"--include-keyframes"],stdout=output,stderr=subprocess.PIPE)
    (args.output_dir/"native.stderr.txt").write_bytes(result.stderr)
    native=json.loads(native_path.read_text(encoding="utf-8"))
    if result.returncode or native["oracle_binary_sha256"]!=binary_hashes[1]:raise AssertionError("authored native source/provenance failed")
    raw={row["file"]:row for row in native["files"]}
    records=[];negatives=[];saved=[]
    def inspect(name,path,block,channel,time,expect_success=True,reason=None):
        output=args.output_dir/f"{name}-rust.json"
        command=[str(args.binary.resolve()),"nif-animation",str(path.resolve()),"--output",str(output.resolve()),"--sample-time",repr(time),"--sample-block",str(block),"--sample-channel",channel]
        result=run(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        (args.output_dir/f"{name}.stdout.txt").write_bytes(result.stdout);(args.output_dir/f"{name}.stderr.txt").write_bytes(result.stderr)
        doc=json.loads(output.read_text()) if output.exists() else None
        if expect_success:
            if result.returncode or doc["failures"]:raise AssertionError(f"sample {name} failed; evidence retained")
            return doc["files"][0]
        errors=result.stderr.decode("utf-8",errors="replace")+json.dumps(doc)
        if not result.returncode or reason not in errors:raise AssertionError(f"{name} accepted or rejected for unrelated reason")
        negatives.append(dict(name=name,exit=result.returncode,intended_reason=reason))
        return doc
    def times(group,original=False):
        keys=group["keys"]
        if not keys:return [0.]
        values=[float(rational32(k["time_bits"])) for k in keys]
        if original:
            values=[values[0],values[-1],values[len(values)//2]]
            pairs=[(keys[0],keys[1])] if len(keys)>1 else []
        else:pairs=zip(keys,keys[1:])
        for left,right in pairs:
            a=rational32(left["time_bits"]);b=rational32(right["time_bits"])
            for alpha in ([F(1,2),F(1,3)] if original else [F(1,4),F(1,3),F(1,2),F(3,4)]):
                values.append(float(a+(b-a)*alpha))
        return list(dict.fromkeys(values))
    for name in CASES:
        row=raw[f"{name}.blob"];block=row["keys"][0]
        for channel in ("translation","scale"):
            group=block["data"]["translations" if channel=="translation" else "scales"]
            for id,time in enumerate(times(group)):
                result=inspect(f"authored-{name}-{channel}-{id}",inputs/f"{name}.blob",0,channel,time)
                metrics=verify(result,row,0,channel,time)
                records.append(dict(scope="authored",file=f"{name}.blob",block=0,channel=channel,time=time,**metrics))
                if name=="ordinary" and channel=="translation":saved.append((time,result,row))
    for name in REFUSALS:
        reason="not strictly increasing" if name in ("duplicate","unsorted") else "tag is unadmitted"
        doc=inspect(f"refusal-{name}",inputs/f"{name}.blob",0,"translation",0.,False,reason)
        source_check(doc["files"][0],raw[f"{name}.blob"])
    for name,time,block,reason in [("extrapolate",99.,0,"extrapolate"),("missing-block",0.,99,"selected block is not in"),("nan",float("nan"),0,"finite explicit source time"),("infinity",float("inf"),0,"finite explicit source time")]:
        inspect(name,inputs/"ordinary.blob",block,"translation",time,False,reason)
    original_raw=json.loads(args.original_native_report.read_text(encoding="utf-8"))
    selected=[]
    for row in original_raw["files"]:
        for block in row["keys"]:
            for channel in ("translation","scale"):
                group=block["data"]["translations" if channel=="translation" else "scales"]
                if group["key_type"] not in (1,5) or not group["keys"]:continue
                source_times=[rational32(k["time_bits"]) for k in group["keys"]]
                if any(a>=b for a,b in zip(source_times,source_times[1:])):continue
                selected.append((row,block,channel,group))
    # Deterministic bounded spread, preserving original groups without repairs.
    chosen=selected[::max(1,len(selected)//16)][:16]
    for id,(row,block,channel,group) in enumerate(chosen):
        for n,time in enumerate(times(group,True)):
            result=inspect(f"original-{id}-{n}",args.original_inputs/row["file"],block["block"],channel,time)
            metrics=verify(result,row,block["block"],channel,time)
            records.append(dict(scope="original",file=row["file"],block=block["block"],channel=channel,time=time,**metrics))
    time,result,row=next(x for x in saved if x[1]["engineering_sample"]["evaluation"]["sample"]["source_value_bits"] is None)
    def sample(d):return d["engineering_sample"]["evaluation"]["sample"]
    changes={"source_hash":lambda d:d["engineering_sample"].update(source_sha256="0"*64),
             "request_time":lambda d:d["engineering_sample"].update(requested_time_f64_bits=0),
             "bracket":lambda d:sample(d).update(source_key_indices=[0,2]),
             "value":lambda d:sample(d)["evaluated_f64_bits"].__setitem__(0,bits64(123.)),
             "runtime_claim":lambda d:d["engineering_sample"].update(runtime_ready=True),
             "mode":lambda d:sample(d).update(interpolation="constant")}
    for name,change in changes.items():
        altered=copy.deepcopy(result);change(altered);write(args.output_dir/f"altered-{name}.json",altered)
        try:verify(altered,row,0,"translation",time)
        except AssertionError as error:negatives.append(dict(name="altered-"+name,intended_reason=str(error)))
        else:raise AssertionError("reference checker accepted altered "+name)
    if binary_hashes!=[digest(args.binary),digest(args.oracle)] or input_hashes!={p.name:digest(p) for p in inputs.iterdir()} or originals!={p.name:digest(p) for p in args.original_inputs.iterdir()}:
        raise AssertionError("binary/authored/original input identity changed")
    summary=dict(contract="engineering-linear-constant-components-v1",reference="independent exact rational binary32 decode; linear interval walk; a+(b-a)*alpha",relative_bound=float(RELATIVE_BOUND),absolute_bound=float(ABSOLUTE_BOUND),bound_scale="abs(left)+abs(right)+abs(exact_result)",max_conditioned_error=max(r["max_conditioned_error"] for r in records),max_absolute_error=max(r["max_absolute_error"] for r in records),sample_requests=len(records),evaluated_values=sum(r["values"] for r in records),authored_files=len(CASES),original_eligible_groups=len(selected),original_selected_groups=len(chosen),original_selected_files=len(set(row["file"] for row,_,_,_ in chosen)),negatives=negatives,samples=records,binaries_sha256=binary_hashes,original_native_report_sha256=digest(args.original_native_report),runtime_ready=False,retail_behavior_verified=False)
    write(args.output_dir/"summary.json",summary)
    print(json.dumps({k:v for k,v in summary.items() if k not in ("samples","negatives")}))

if __name__=="__main__":main()
