"""Frozen pose consumer, independently authored packets and analytic matrices.

This writes bounded authored NIF bytes; it never introduces another decoder.
Clocks and raw quaternions remain observations, without retail playback claims.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

NULL = 2**32-1
def words(*values):
    return struct.pack('<'+'I'*len(values), *values)
def floats(*values):
    return struct.pack('<'+'f'*len(values), *values)
def node(controller, children, translation, rotation, scale):
    return (words(NULL,0,controller,0x89ABCDEF)+floats(*translation,*rotation,scale)
            +words(0,NULL,len(children),*children,0))
def fixture(target=1, ancestor=NULL, chain=NULL):
    packets = [
        node(ancestor,[1],[2,-3,5],[0,-1,0,1,0,0,0,0,1],2),
        node(2,[],[11,12,13],[-1,0,0,0,-1,0,0,0,1],6),
        words(chain)+struct.pack('<H',0xFFFF)+floats(9,22,50,51)+words(target,3),
        floats(100,200,300,2,-3,4,-5,12)+words(4),
        words(0,2,1)+floats(-1,4,0,-2,3,0,8,6)+words(2,1)+floats(-1,-2,3,2),
    ]
    names=['NiNode','NiTransformController','NiTransformInterpolator','NiTransformData']
    out=(b'Gamebryo File Format, Version 20.2.0.7\n'+words(0x14020007)
         +b'\x01'+words(11,5,34)+b'\x00'*3+struct.pack('<H',4))
    for name in names:
        out+=words(len(name))+name.encode('ascii')
    out+=struct.pack('<5H',0,0,1,2,3)+words(*(len(packet) for packet in packets))
    return out+words(0,0,0)+b''.join(packets)+words(1,0)
def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',required=True,type=Path)
    parser.add_argument('--output-dir',required=True,type=Path)
    args=parser.parse_args()
    args.output_dir.mkdir(exist_ok=False)
    inputs=args.output_dir/'inputs'; inputs.mkdir()
    binary=args.binary.resolve(); before=digest(binary)
    source=inputs/'authored.nif'; source.write_bytes(fixture())
    invocations=[]
    def invoke(name,input_path,controller,time,error=None):
        receipt=args.output_dir/(name+'.json')
        command=[str(binary),'nif-source-pose',str(input_path.resolve()),'--object','1','--controller',str(controller),
                 '--source-time',str(time),'--output',str(receipt.resolve())]
        run=subprocess.run(command,capture_output=True,text=True)
        (args.output_dir/(name+'.stderr.txt')).write_text(run.stderr)
        if run.returncode != (1 if error else 0):
            raise ValueError(f'{name}: unexpected exit {run.returncode}: {run.stderr}')
        value=json.loads(receipt.read_text())
        if value['sha256'] != digest(input_path) or value['contract'] != 'engineering-linked-source-pose-v1':
            raise ValueError('whole source receipt identity differs')
        if error:
            if value['failures'] != 1 or value['evaluation'] is not None or error not in value['error']:
                raise ValueError(f'{name}: intended contextual refusal missing')
        elif value['failures'] != 0 or value['evaluation']['retail_behavior_verified'] is not False:
            raise ValueError('engineering pose receipt missing')
        invocations.append(dict(command=command,exit_code=run.returncode,receipt_sha256=digest(receipt),expected_error=error))
        return value['evaluation']
    # Independent literal expectations: parent quarter-turn/scale2, child
    # half-turn and linearly sampled signed scale -2..2. Source clocks disagree.
    cases=[
        ('first',-1,[[2,0,0,4],[0,2,0,0],[0,0,-2,-2]],[[0,-4,0,2],[4,0,0,5],[0,0,-4,1]]),
        ('quarter',0,[[1,0,0,3],[0,1,0,2],[0,0,-1,0]],[[0,-2,0,-2],[2,0,0,3],[0,0,-2,5]]),
        ('zero-scale',1,[[0,0,0,2],[0,0,0,4],[0,0,0,2]],[[0,0,0,-6],[0,0,0,1],[0,0,0,9]]),
        ('positive-scale',1.5,[[-.5,0,0,1.5],[0,-.5,0,5],[0,0,.5,3]],[[0,1,0,-8],[-1,0,0,0],[0,0,1,11]]),
        ('last',3,[[-2,0,0,0],[0,-2,0,8],[0,0,2,6]],[[0,4,0,-14],[-4,0,0,-3],[0,0,4,17]]),
    ]
    for name,time,local,world in cases:
        result=invoke(name,source,2,time)
        if result['local'] != local or result['source_world'] != world:
            raise ValueError(f'{name}: independent analytic matrix differs')
        raw=[struct.unpack('<I',floats(value))[0] for value in [2,-3,4,-5]]
        if (result['source_sha256'] != digest(source) or result['unapplied_controller_fields']['flags'] != 0xFFFF
            or result['unapplied_interpolator_fields']['rotation_wxyz_bits'] != raw):
            raise ValueError('raw source observations changed')
    invoke('nonfinite',source,2,'NaN','must be finite')
    invoke('before-range',source,2,-2,'extrapolate')
    invoke('after-range',source,2,4,'extrapolate')
    invoke('wrong-controller',source,0,1,'object.controller differs')
    for name,data,error in [
        ('wrong-target',fixture(target=0),'controller.target differs'),
        ('chain',fixture(chain=2),'controller chain'),
        ('ancestor-controller',fixture(ancestor=2),'ancestor 0 controller is unapplied'),
    ]:
        altered=inputs/(name+'.nif'); altered.write_bytes(data)
        invoke(name,altered,2,1,error)
    if digest(binary) != before or source.read_bytes() != fixture():
        raise ValueError('frozen binary/source changed')
    summary=dict(contract='engineering-linked-source-pose-v1',binary_sha256=before,source_sha256=digest(source),
                 analytic_cases=len(cases),intended_refusals=7,invocations=invocations,retail_behavior_verified=False)
    (args.output_dir/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps(summary))
if __name__ == '__main__':
    main()
