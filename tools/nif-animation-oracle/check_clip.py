"""Independent two-source clip packets and literal explicit-time pose checks.

No decoder or production interpolation/hierarchy routine is reproduced here.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

NULL=2**32-1
NAME=b'ChosenNode\x00\xfe'
CONTRACT='engineering-exact-external-clip-pose-v1'
def words(*values):return struct.pack('<'+'I'*len(values),*values)
def floats(*values):return struct.pack('<'+'f'*len(values),*values)
def node(name,children,translation,rotation,scale,controller=NULL):
    return words(name,0,controller,0xA9)+floats(*translation,*rotation,scale)+words(0,NULL,len(children),*children,0)
def container(names,packets,type_indices,strings):
    out=b'Gamebryo File Format, Version 20.2.0.7\n'+words(0x14020007)+b'\x01'+words(11,len(packets),34)+b'\x00'*3+struct.pack('<H',len(names))
    for name in names:out+=words(len(name))+name.encode('ascii')
    out+=struct.pack('<'+'H'*len(type_indices),*type_indices)+words(*(len(p) for p in packets))+words(len(strings),max(map(len,strings)))
    for string in strings:out+=words(len(string))+string
    return out+words(0)+b''.join(packets)+words(1,0)
def skeleton(duplicate=False,ancestor=False):
    return container(['NiNode'],[
        node(0,[1],[2,-3,5],[-1,0,0,0,-1,0,0,0,1],3,0 if ancestor else NULL),
        node(1,[],[1,4,-2],[0,-1,0,1,0,0,0,0,1],2)], [0,0],[NAME if duplicate else b'root',NAME])
def clip(packet_name=0,controller_type=1,rotation=False):
    sequence=(words(NULL,1,9,1,NULL)+bytes([41])+words(packet_name,NULL,controller_type,NULL,NULL)
              +floats(-3)+words(NULL,2)+floats(13,100,101)+words(NULL,NULL)+struct.pack('<H',0))
    interpolator=floats(100,200,300,2,3,4,5,16)+words(2)
    rotation_keys=words(1,1)+floats(0,1,0,0,0) if rotation else words(0)
    data=rotation_keys+words(2,1)+floats(-2,8,-4,2,2,-4,12,10)+words(2,1)+floats(-2,-1,2,3)
    return container(['NiControllerSequence','NiTransformInterpolator','NiTransformData'],[sequence,interpolator,data],[0,1,2],[NAME,b'NiTransformController',b'other'])
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',required=True,type=Path);parser.add_argument('--output-dir',required=True,type=Path)
    args=parser.parse_args();args.output_dir.mkdir(exist_ok=False);inputs=args.output_dir/'inputs';inputs.mkdir()
    binary=args.binary.resolve();before=digest(binary);s=inputs/'skeleton.nif';s.write_bytes(skeleton());c=inputs/'clip.kf';c.write_bytes(clip())
    request=dict(schema_version=1,expected_skeleton_sha256=list(bytes.fromhex(digest(s))),expected_clip_sha256=list(bytes.fromhex(digest(c))),
                 object=1,node_name_bytes=list(NAME),sequence=0,controlled_ordinal=0,source_time=0)
    invocations=[]
    def invoke(name,time,patch=None,skeleton_path=s,clip_path=c,local=None,world=None,error=None,transport=None):
        req={**request,'expected_skeleton_sha256':list(bytes.fromhex(digest(skeleton_path))),
             'expected_clip_sha256':list(bytes.fromhex(digest(clip_path))),'source_time':time,**(patch or {})}
        req_path=inputs/(name+'.request.json');req_path.write_text(json.dumps(req)+'\n');out=args.output_dir/(name+'.json')
        command=[str(binary),'nif-clip-pose',str(skeleton_path.resolve()),str(clip_path.resolve()),'--request',str(req_path.resolve()),'--output',str(out.resolve())]
        run=subprocess.run(command,capture_output=True,text=True);(args.output_dir/(name+'.stderr.txt')).write_text(run.stderr)
        if run.returncode!=(1 if error else 0):raise ValueError(f'{name}: exit{run.returncode}: {run.stderr}')
        report=json.loads(out.read_text())
        if report['contract']!=CONTRACT or report['skeleton_sha256']!=digest(skeleton_path) or report['clip_sha256']!=digest(clip_path) or report['request_sha256']!=digest(req_path):raise ValueError('source identity differs')
        result=report['evaluation']
        if error:
            if report['failures']!=1 or result is not None or error not in report['error']:raise ValueError(f'{name}: intended contextual refusal missing')
        else:
            if report['failures']!=0 or result['retail_behavior_verified'] is not False:raise ValueError('engineering result missing')
            if local is not None and (result['local']!=local or result['source_world']!=world):raise ValueError(f'{name}: literal affine expectation differs')
            if result['requested_time_f64_bits']!=struct.unpack('<Q',struct.pack('<d',time))[0]:raise ValueError('explicit binary64 time word differs')
            if transport is not None and result['requested_time_f64_bits']!=transport:raise ValueError('prescribed transport regression word differs')
            if result['controlled_packet']['priority']!=41 or result['unapplied_interpolator_fields']['rotation_wxyz_bits']!=[struct.unpack('<I',floats(v))[0] for v in [2,3,4,5]]:raise ValueError('raw packet/quaternion fields changed')
        invocations.append(dict(command=command,exit_code=run.returncode,receipt_sha256=digest(out),expected_error=error))
    cases=[
        ('first',-2,[[0,1,0,8],[-1,0,0,-4],[0,0,-1,2]],[[0,-3,0,-22],[3,0,0,9],[0,0,-3,11]]),
        ('zero-scale',-1,[[0,0,0,5],[0,0,0,0],[0,0,0,4]],[[0,0,0,-13],[0,0,0,-3],[0,0,0,17]]),
        ('midpoint',0,[[0,-1,0,2],[1,0,0,4],[0,0,1,6]],[[0,3,0,-4],[-3,0,0,-15],[0,0,3,23]]),
        ('positive',.5,[[0,-1.5,0,.5],[1.5,0,0,6],[0,0,1.5,7]],[[0,4.5,0,.5],[-4.5,0,0,-21],[0,0,4.5,26]]),
        ('last',2,[[0,-3,0,-4],[3,0,0,12],[0,0,3,10]],[[0,9,0,14],[-9,0,0,-39],[0,0,9,35]]),
    ]
    for name,time,local,world in cases:invoke(name,time,local=local,world=world)
    invoke('transport-word',0.23005015640173943,transport=0x3fcd72489517b330)
    for name,time,patch,error in [
        ('stale-skeleton',0,{'expected_skeleton_sha256':[0]*32},'skeleton source SHA256 differs'),
        ('stale-clip',0,{'expected_clip_sha256':[0]*32},'clip source SHA256 differs'),
        ('wrong-object',0,{'object':0},'skeleton raw node name differs'),
        ('wrong-name',0,{'node_name_bytes':[1,2,3]},'skeleton raw node name differs'),
        ('wrong-sequence',0,{'sequence':1},'not decoded NiControllerSequence'),
        ('wrong-ordinal',0,{'controlled_ordinal':1},'ordinal is out of range'),
        ('before-range',-3,None,'extrapolate'),('after-range',3,None,'extrapolate'),
    ]:invoke(name,time,patch=patch,error=error)
    for name,data,error in [('duplicate-name',skeleton(duplicate=True),'not unique'),('ancestor',skeleton(ancestor=True),'ancestor 0 controller is unapplied')]:
        path=inputs/(name+'.nif');path.write_bytes(data);invoke(name,0,skeleton_path=path,error=error)
    for name,data,error in [('packet-name',clip(packet_name=2),'packet raw node name differs'),('packet-type',clip(controller_type=2),'transform-controller type is unavailable'),
                            ('rotation',clip(rotation=True),'rotation key mapping is unapplied')]:
        path=inputs/(name+'.kf');path.write_bytes(data);invoke(name,0,clip_path=path,error=error)
    if digest(binary)!=before or s.read_bytes()!=skeleton() or c.read_bytes()!=clip():raise ValueError('frozen inputs or binary changed')
    summary=dict(contract=CONTRACT,binary_sha256=before,analytic_cases=len(cases),binary64_transport_cases=1,intended_refusals=13,invocations=invocations,retail_behavior_verified=False)
    (args.output_dir/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({key:summary[key] for key in ['contract','binary_sha256','analytic_cases','binary64_transport_cases','intended_refusals']}))
if __name__=='__main__':main()
