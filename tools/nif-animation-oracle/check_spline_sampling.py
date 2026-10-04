"""Independent rational cubic component checks over a frozen native source report."""
import argparse
import copy
from fractions import Fraction as F
from functools import lru_cache
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
from process_guard import guard, run

CONTRACT = "engineering-open-uniform-cubic-components-v1"
RELATIVE = F(1, 1 << 42)
ABSOLUTE = F(1, 1 << 1074)
WIDTHS = dict(translation=3, scale=1, float=1, point3=3, rotation_components=4)

def write(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, indent=2); output.write("\n")

def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(1048576): h.update(block)
    return h.hexdigest()

def bounded(path, limit):
    with path.open("rb") as source: data = source.read(limit + 1)
    if len(data) > limit: raise AssertionError("reference input exceeds byte limit")
    return json.loads(data)

def bits64(value): return struct.unpack("<Q", struct.pack("<d", value))[0]
def float64(word): return struct.unpack("<d", struct.pack("<Q", word))[0]
def rational32(word):
    sign = -1 if word >> 31 else 1
    exponent = (word >> 23) & 255; mantissa = word & 0x7fffff
    assert exponent != 255
    numerator = sign * (mantissa + (1 << 23) if exponent else mantissa)
    power = exponent - 150 if exponent else -149
    return F(numerator * (1 << power)) if power >= 0 else F(numerator, 1 << -power)

def knot(index, count): return 0 if index <= 3 else min(index - 3, count - 3)

def reference(raw, count, width, offset, half, start, stop, time):
    """Exact Cox basis, using its local support and an independently exact span."""
    parameter = (F.from_float(time) - start) / (stop - start) * (count - 3)
    assert 0 <= parameter <= count - 3
    if parameter == count - 3: weights = [(count - 1, F(1))]
    else:
        @lru_cache(None)
        def basis(index, degree):
            if degree == 0: return F(int(knot(index, count) <= parameter < knot(index + 1, count)))
            value = F(0)
            left = knot(index + degree, count) - knot(index, count)
            right = knot(index + degree + 1, count) - knot(index + 1, count)
            if left: value += (parameter - knot(index, count)) / left * basis(index, degree - 1)
            if right: value += (knot(index + degree + 1, count) - parameter) / right * basis(index + 1, degree - 1)
            return value
        weights = [(i, basis(i, 3)) for i in range(int(parameter), min(count, int(parameter) + 4)) if basis(i, 3)]
    assert sum(w for _, w in weights) == 1
    values = [sum(w * (offset + F(raw[i * width + c], 32767) * half) for i, w in weights) for c in range(width)]
    if count == 4:
        t = parameter
        bernstein = [(1-t)**3, 3*t*(1-t)**2, 3*t*t*(1-t), t**3]
        assert values == [sum(bernstein[i] * (offset + F(raw[i*width+c],32767)*half) for i in range(4)) for c in range(width)]
    return parameter, values

def exact(actual, expected, reason):
    if json.dumps(actual, sort_keys=True, separators=(",", ":")) != json.dumps(expected, sort_keys=True, separators=(",", ":")):
        raise AssertionError(reason)

def source_check(row, native):
    exact([row["sha256"], row["decoded_bytes"], row["animation"]["blocks"], row["keys"]["blocks"],
           row["splines"]["blocks"], row["spline_components"]["blocks"]],
          [native["sha256"], native["decoded_bytes"], native["animations"], native["keys"],
           native["splines"], native["spline_components"]], "source identity/ordered component projection differs")

def verify(row, native, request):
    source_check(row, native)
    channel = request["channel"]; width = WIDTHS[channel]
    blocks = native["splines"] + native["spline_components"]
    interpolator = next(b for b in blocks if b["block"] == request["block"])
    fields = interpolator["data"]
    data = next(b for b in native["splines"] if b["block"] == fields["spline_data"])
    basis = next(b for b in native["splines"] if b["block"] == fields["basis_data"])
    count = basis["data"]["num_control_points"]
    prefix = "rotation" if channel == "rotation_components" else "position" if channel == "point3" else channel
    handle = fields[channel + "_handle"] if channel in ("translation", "scale") else fields["rotation_handle"] if channel == "rotation_components" else fields["handle"]
    assert handle != 65535 and 4 <= count <= 2_000_000
    raw = data["data"]["compact"][handle:handle + count*width]
    assert len(raw) == count*width
    offset_bits = fields[prefix + "_offset_bits"]; half_bits = fields[prefix + "_half_range_bits"]
    offset, half, start, stop = map(rational32, (offset_bits,half_bits,fields["start_bits"],fields["stop_bits"]))
    parameter, values = reference(raw,count,width,offset,half,start,stop,request["time"])
    diagnostic = row["engineering_spline_sample"]
    identities = [{k:b[k] for k in ("block","block_type","offset","bytes","sha256")} for b in (interpolator,data,basis)]
    expected = dict(contract=CONTRACT, source_blocks=identities, channel=channel, basis_count=count,
        source_handle=handle, window_offset=data["offset"]+8+4*len(data["data"]["float_bits"])+2*handle,
        window_scalars=count*width, window_sha256=hashlib.sha256(struct.pack("<"+"h"*len(raw),*raw)).hexdigest(),
        start_bits=fields["start_bits"], stop_bits=fields["stop_bits"], offset_bits=offset_bits,
        half_range_bits=half_bits, runtime_ready=False, retail_behavior_verified=False)
    exact({k:diagnostic[k] for k in expected},expected,"diagnostic identity/window/parameters/contract differ")
    sample = diagnostic["sample"]
    exact(sample["requested_time_f64_bits"],bits64(request["time"]),"requested time differs")
    actual_parameter = float64(sample["parameter_f64_bits"])
    assert math.isfinite(actual_parameter)
    if abs(F.from_float(actual_parameter)-parameter) > F(1,1<<48)*max(1,count-3)+ABSOLUTE:
        raise AssertionError("normalized parameter differs")
    span = min(count-1, int(actual_parameter)+3)
    exact(sample["source_control_indices"],list(range(span-3,span+1)),"reported local source controls differ")
    if len(sample["evaluated_f64_bits"]) != width: raise AssertionError("component cardinality differs")
    validation = 16 + len(native["splines"]) * 3 + len(native["spline_components"]) + count*width
    exact(diagnostic["work"],dict(validation_units=validation,sampling_units=17+10*width),"admitted work receipt differs")
    maximum = F(0)
    for word, expected_value in zip(sample["evaluated_f64_bits"],values):
        actual = float64(word)
        if not math.isfinite(actual): raise AssertionError("nonfinite evaluated component")
        error = abs(F.from_float(actual)-expected_value)
        scale = abs(offset)+F(32768,32767)*abs(half)+abs(expected_value)
        if error > RELATIVE*max(1,count-3)*scale+ABSOLUTE: raise AssertionError("component exceeds rational-reference bound")
        maximum = max(maximum,error/scale if scale else F(0))
    return dict(values=width,basis_count=count,max_conditioned_error=float(maximum))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--source-inputs",type=Path,required=True)
    parser.add_argument("--native-report",type=Path,required=True)
    parser.add_argument("--requests",type=Path,required=True)
    parser.add_argument("--output-dir",type=Path,required=True)
    args=parser.parse_args(); guard(); args.output_dir.mkdir(parents=True,exist_ok=False)
    document=bounded(args.native_report,128*1024*1024)
    assert document["schema_version"] == 6 and document["runtime_ready"] is False and document["prepare_data_called"] is False
    assert document["nifly_revision"] == "cca0a770094bb962fb28ea1fec5ea903e68fda8e"
    assert document["raw_spline_counts_checked"] is True and document["raw_component_fields_checked"] is True
    assert document["float_encoding"] == "ieee754-binary32-bits"
    native={r["file"]:r for r in document["files"]}
    requests=bounded(args.requests,4*1024*1024)
    if len(requests)>10_000: raise AssertionError("request count exceeds budget")
    binary_hash=digest(args.binary); native_hash=digest(args.native_report)
    names={r["file"] for r in requests}
    if any(Path(name).name != name for name in names): raise AssertionError("source name escapes input directory")
    if any((args.source_inputs/name).stat().st_size>64*1024*1024 for name in names): raise AssertionError("source exceeds byte limit")
    sources={name:digest(args.source_inputs/name) for name in names}
    records=[]; refusals=[]; saved=None
    for i,request in enumerate(requests):
        guard()
        if Path(request["file"]).name!=request["file"]: raise AssertionError("source name escapes input directory")
        output=args.output_dir/f"request-{i:04}-rust.json"
        command=[str(args.binary.resolve()),"nif-animation",str((args.source_inputs/request["file"]).resolve()),
            "--include-bool-keys","--sample-block",str(request["block"]),"--sample-channel","spline-"+request["channel"].replace("_","-"),
            "--sample-time",repr(request["time"]),"--output",str(output.resolve())]
        result=run(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        (args.output_dir/f"request-{i:04}.stderr.txt").write_bytes(result.stderr)
        report=bounded(output,128*1024*1024) if output.exists() else None
        if request.get("refusal"):
            errors=result.stderr.decode("utf-8",errors="replace")+json.dumps(report)
            if not result.returncode or request["refusal"] not in errors: raise AssertionError(f"request {i} accepted or unrelated refusal")
            if report:
                source_check(report["files"][0],native[request["file"]])
                assert "engineering_spline_sample" not in report["files"][0]
            refusals.append(dict(request=i,reason=request["refusal"]))
        else:
            if result.returncode or report["failures"]: raise AssertionError(f"request {i} failed; evidence retained")
            assert report["engineering_spline_sampling_contract"] == CONTRACT
            metrics=verify(report["files"][0],native[request["file"]],request)
            records.append(dict(request=i,file=request["file"],channel=request["channel"],time=request["time"],**metrics))
            if saved is None and 0<request["time"]<1:saved=(report["files"][0],native[request["file"]],request)
    altered=[]
    if saved:
        row,raw,request=saved
        changes={"window_hash":lambda d:d["engineering_spline_sample"].update(window_sha256="0"*64),
            "source_order":lambda d:d["engineering_spline_sample"]["source_blocks"].reverse(),
            "source_type":lambda d:d["engineering_spline_sample"].update(source_handle=True),
            "time":lambda d:d["engineering_spline_sample"]["sample"].update(requested_time_f64_bits=0),
            "span":lambda d:d["engineering_spline_sample"]["sample"].update(source_control_indices=[0,0,0,0]),
            "value":lambda d:d["engineering_spline_sample"]["sample"]["evaluated_f64_bits"].__setitem__(0,bits64(123.)),
            "work":lambda d:d["engineering_spline_sample"]["work"].update(sampling_units=0),
            "retail":lambda d:d["engineering_spline_sample"].update(retail_behavior_verified=True)}
        for name,change in changes.items():
            candidate=copy.deepcopy(row);change(candidate);write(args.output_dir/f"altered-{name}.json",candidate)
            try: verify(candidate,raw,request)
            except AssertionError as error: altered.append(dict(name=name,reason=str(error)))
            else: raise AssertionError("reference accepted altered "+name)
    assert digest(args.binary)==binary_hash and digest(args.native_report)==native_hash
    assert sources=={name:digest(args.source_inputs/name) for name in sources}
    summary=dict(contract=CONTRACT,reference="exact rational binary32/binary64 Cox-de Boor basis; N4 Bernstein equality",
        bound="2^-42 * max(1,N-3) * (abs(offset) + (32768/32767)*abs(half_range) + abs(exact_result)) + 2^-1074",
        sample_requests=len(records),values=sum(r["values"] for r in records),refusals=refusals,altered=altered,
        max_conditioned_error=max((r["max_conditioned_error"] for r in records),default=0),records=records,
        source_hashes=sources,binary_sha256=binary_hash,native_report_sha256=native_hash,runtime_ready=False,retail_behavior_verified=False)
    write(args.output_dir/"summary.json",summary)
    print(json.dumps({k:v for k,v in summary.items() if k not in ("records","source_hashes","refusals","altered")}))

if __name__=="__main__": main()
