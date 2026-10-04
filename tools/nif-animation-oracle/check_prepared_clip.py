"""Independent two-source literals, atomic refusal and old one-shot byte checks.

Uses the separate authored packet writer; no importer, sampler or matrix product.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import check_clip as source

CONTRACT='engineering-prepared-two-source-clip-batch-v1'
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def normalized(value):
    value=copy.deepcopy(value);value.pop('retained_bytes');value.pop('work_units');return value
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',required=True,type=Path);parser.add_argument('--prior-binary',required=True,type=Path)
    parser.add_argument('--output-dir',required=True,type=Path);args=parser.parse_args()
    args.output_dir.mkdir(exist_ok=False);inputs=args.output_dir/'inputs';inputs.mkdir()
    s=inputs/'skeleton.nif';s.write_bytes(source.skeleton());c=inputs/'clip.kf';c.write_bytes(source.clip())
    binaries=[args.binary.resolve(),args.prior_binary.resolve()];frozen=[digest(path) for path in binaries]
    request=dict(schema_version=1,expected_skeleton_sha256=list(bytes.fromhex(digest(s))),expected_clip_sha256=list(bytes.fromhex(digest(c))),
        object=1,node_name_bytes=list(source.NAME),sequence=0,controlled_ordinal=0)
    commands=[]
    def invoke(name,times,patch=None,skeleton=s,clip=c,error=None):
        req={**request,'source_times':times,'expected_skeleton_sha256':list(bytes.fromhex(digest(skeleton))),
            'expected_clip_sha256':list(bytes.fromhex(digest(clip))),**(patch or {})}
        path=inputs/(name+'.request.json');path.write_text(json.dumps(req)+'\n');out=args.output_dir/(name+'.json')
        command=[str(binaries[0]),'nif-clip-pose',str(skeleton),'--batch',str(clip),'--request',str(path),'--output',str(out)]
        run=subprocess.run(command,capture_output=True,text=True);(args.output_dir/(name+'.stderr.txt')).write_text(run.stderr)
        if run.returncode!=(1 if error else 0):raise ValueError(f'{name}: exit{run.returncode}: {run.stderr}')
        report=json.loads(out.read_text())
        if report['contract']!=CONTRACT or report['skeleton_sha256']!=digest(skeleton) or report['clip_sha256']!=digest(clip) or report['request_sha256']!=digest(path):raise ValueError('exact source identity differs')
        if error:
            if report['failures']!=1 or report['evaluation'] is not None or error not in report['error']:raise ValueError(f'{name}: atomic contextual refusal missing')
        elif report['failures'] or report['evaluation']['retail_behavior_verified'] is not False:raise ValueError('engineering-only batch missing')
        commands.append(dict(command=command,exit_code=run.returncode,expected_error=error,receipt_sha256=digest(out)))
        return report['evaluation']
    times=[2,-2,0,-0.0,.5,-1,2]
    literal_local=[
        [[0,-3,0,-4],[3,0,0,12],[0,0,3,10]],
        [[0,1,0,8],[-1,0,0,-4],[0,0,-1,2]],
        [[0,-1,0,2],[1,0,0,4],[0,0,1,6]],
        [[0,-1,0,2],[1,0,0,4],[0,0,1,6]],
        [[0,-1.5,0,.5],[1.5,0,0,6],[0,0,1.5,7]],
        [[0,0,0,5],[0,0,0,0],[0,0,0,4]],
        [[0,-3,0,-4],[3,0,0,12],[0,0,3,10]],
    ]
    literal_world=[
        [[0,9,0,14],[-9,0,0,-39],[0,0,9,35]],
        [[0,-3,0,-22],[3,0,0,9],[0,0,-3,11]],
        [[0,3,0,-4],[-3,0,0,-15],[0,0,3,23]],
        [[0,3,0,-4],[-3,0,0,-15],[0,0,3,23]],
        [[0,4.5,0,.5],[-4.5,0,0,-21],[0,0,4.5,26]],
        [[0,0,0,-13],[0,0,0,-3],[0,0,0,17]],
        [[0,9,0,14],[-9,0,0,-39],[0,0,9,35]],
    ]
    batch=invoke('ordered',times);repeat=invoke('repeated',times)
    if batch!=repeat or len(batch['samples'])!=len(times):raise ValueError('ordered deterministic reusable output differs')
    prep=batch['preparation']
    for key,value in [('animation_key_decodes',1),('scene_decodes',1),('target_bindings',1),('whole_source_sha256_computations',2),('additional_span_hashes',5)]:
        if prep[key]!=value:raise ValueError('preparation did not record one binding/decode')
    old_equal=[]
    for ordinal,(time,entry) in enumerate(zip(times,batch['samples'])):
        if entry['local']!=literal_local[ordinal] or entry['source_world']!=literal_world[ordinal]:raise ValueError('literal pose differs')
        if entry['requested_time_f64_bits']!=struct.unpack('<Q',struct.pack('<d',time))[0]:raise ValueError('signed time word/order differs')
        # Authored fixed-size packet suffixes supply independent exact spans;
        # this does not decode retail fields or repeat the production importer.
        expected=[(s,entry['object'],len(s.read_bytes())-8-84,84),(s,entry['static_ancestors'][0]['source'],len(s.read_bytes())-8-84-88,88),
                  (c,entry['sequence'],len(c.read_bytes())-8-68-36-75,75),(c,entry['interpolator'],len(c.read_bytes())-8-68-36,36),(c,entry['data'],len(c.read_bytes())-8-68,68)]
        for path,span,offset,size in expected:
            if span['offset']!=offset or span['bytes']!=size or span['sha256']!=hashlib.sha256(path.read_bytes()[offset:offset+size]).hexdigest():raise ValueError('cached literal packet span differs')
        one_path=inputs/(f'one-{ordinal}.request.json');one_path.write_text(json.dumps({**request,'source_time':time})+'\n')
        outputs=[]
        for label,binary in zip(['prior','current'],binaries[::-1]):
            out=args.output_dir/(f'one-{ordinal}-{label}.json');command=[str(binary),'nif-clip-pose',str(s),str(c),'--request',str(one_path),'--output',str(out)]
            run=subprocess.run(command,capture_output=True,text=True)
            if run.returncode:raise ValueError(run.stderr)
            outputs.append(out);commands.append(dict(command=command,exit_code=run.returncode,receipt_sha256=digest(out)))
        if outputs[0].read_bytes()!=outputs[1].read_bytes():raise ValueError('old complete one-shot report changed')
        old=json.loads(outputs[0].read_text())['evaluation']
        if normalized(entry)!=normalized(old):raise ValueError('prepared full source/numeric observations differ')
        if entry['work_units']>=old['work_units']:raise ValueError('binding work repeated during sample')
        old_equal.append(digest(outputs[0]))
    invoke('empty',[],error='nonempty bounded time list')
    for name,patch,error in [('stale-skeleton',{'expected_skeleton_sha256':[0]*32},'skeleton source SHA256 differs'),
        ('stale-clip',{'expected_clip_sha256':[0]*32},'clip source SHA256 differs'),('wrong-name',{'node_name_bytes':[1,2,3]},'raw node name differs'),
        ('wrong-object',{'object':0},'raw node name differs'),('wrong-sequence',{'sequence':1},'not decoded NiControllerSequence'),
        ('wrong-ordinal',{'controlled_ordinal':1},'ordinal is out of range')]:invoke(name,[0],patch,error=error)
    invoke('late-before',[0,2,-3],error='extrapolate');invoke('late-after',[0,2,3],error='extrapolate')
    for name,data,error in [('duplicate',source.skeleton(duplicate=True),'not unique'),('ancestor',source.skeleton(ancestor=True),'ancestor 0 controller is unapplied')]:
        path=inputs/(name+'.nif');path.write_bytes(data);invoke(name,[0],skeleton=path,error=error)
    for name,data,error in [('packet-name',source.clip(packet_name=2),'packet raw node name differs'),('packet-type',source.clip(controller_type=2),'transform-controller type is unavailable'),('rotation',source.clip(rotation=True),'rotation key mapping is unapplied')]:
        path=inputs/(name+'.kf');path.write_bytes(data);invoke(name,[0],clip=path,error=error)
    if [digest(path) for path in binaries]!=frozen or s.read_bytes()!=source.skeleton() or c.read_bytes()!=source.clip():raise ValueError('frozen artifacts changed')
    summary=dict(contract=CONTRACT,literal_ordered_samples=7,intended_refusals=14,prior_complete_one_shot_byte_equal=old_equal,
        preparation=prep,batch_retained_bytes=batch['retained_bytes'],batch_work_units=batch['work_units'],batch_sample_work=batch['sample_work'],
        binaries=[dict(path=str(path),sha256=sha) for path,sha in zip(binaries,frozen)],invocations=commands,retail_behavior_verified=False)
    (args.output_dir/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({key:summary[key] for key in ['contract','literal_ordered_samples','intended_refusals','preparation','batch_retained_bytes','batch_work_units','batch_sample_work']}))
if __name__=='__main__':main()
