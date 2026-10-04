"""Independently authored two-source attachment CLI and literal affine results.

This creates bounded packets; it neither decodes sources nor borrows the
production matrix/sampling implementation. Stored locals are engineering data.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

NULL=2**32-1
NAME=b'WeaponSocket\x00\xfe'
CONTRACT='engineering-source-local-rigid-attachment-v1'

def words(*values): return struct.pack('<'+'I'*len(values),*values)
def floats(*values): return struct.pack('<'+'f'*len(values),*values)
def node(name,children,translation,rotation,scale):
    return words(name,0,NULL,0x8ABCDEF0)+floats(*translation,*rotation,scale)+words(0,NULL,len(children),*children,0)
def fixture(packets,strings):
    out=(b'Gamebryo File Format, Version 20.2.0.7\n'+words(0x14020007)+b'\x01'
         +words(11,len(packets),34)+b'\x00'*3+struct.pack('<H',1)+words(6)+b'NiNode')
    out+=struct.pack('<'+'H'*len(packets),*([0]*len(packets)))+words(*(len(p) for p in packets))
    out+=words(len(strings),max(map(len,strings)))
    for value in strings:out+=words(len(value))+value
    return out+words(0)+b''.join(packets)+words(1,0)
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',required=True,type=Path)
    parser.add_argument('--output-dir',required=True,type=Path)
    args=parser.parse_args();args.output_dir.mkdir(exist_ok=False)
    inputs=args.output_dir/'inputs';inputs.mkdir()
    for name in ['skeleton','attachment','requests']:(inputs/name).mkdir()
    binary=args.binary.resolve();before=digest(binary)
    skeleton=inputs/'skeleton/source.nif';attachment=inputs/'attachment/source.nif'
    skeleton.write_bytes(fixture([
        node(0,[1],[2,-3,5],[-1,0,0,0,-1,0,0,0,1],3),
        node(1,[],[1,4,-2],[0,-1,0,1,0,0,0,0,1],2)], [b'root',NAME]))
    attachment.write_bytes(fixture([node(0,[],[-5,8,10],[-1,0,0,0,-1,0,0,0,1],-2)],[b'root']))
    hashes={str(path):digest(path) for path in [binary,skeleton,attachment]}
    request=dict(schema_version=1,expected_skeleton_sha256=list(bytes.fromhex(digest(skeleton))),
                 expected_attachment_sha256=list(bytes.fromhex(digest(attachment))),node=1,node_name_bytes=list(NAME),attachment_root=0,
                 attachment_parent_to_node=[[1,2,0,3],[0,-1,0,-4],[0,0,.5,7]],source_policy='stored_ni_av_locals')
    invocations=[]
    def invoke(name,req,mapping=None,root=None,error=None,parse_error=False,protected=None):
        request_path=inputs/'requests'/(name+'.json');request_path.write_text(json.dumps(req)+'\n')
        output=(protected/'blocked.json') if protected else args.output_dir/(name+'.json')
        command=[str(binary),'nif-rigid-attachment',str(skeleton.resolve()),str(attachment.resolve()),'--request',str(request_path.resolve()),'--output',str(output.resolve())]
        run=subprocess.run(command,capture_output=True,text=True)
        (args.output_dir/(name+'.stderr.txt')).write_text(run.stderr)
        if run.returncode!=(1 if error else 0):raise ValueError(f'{name}: exit{run.returncode}: {run.stderr}')
        if parse_error or protected:
            if output.exists() or error not in run.stderr:raise ValueError(f'{name}: refusal before publication missing')
        else:
            report=json.loads(output.read_text())
            if report['skeleton_sha256']!=digest(skeleton) or report['attachment_sha256']!=digest(attachment) or report['request_sha256']!=digest(request_path):raise ValueError('receipt identity differs')
            if report['contract']!=CONTRACT:raise ValueError('receipt contract differs')
            result=report['evaluation']
            if error:
                if report['failures']!=1 or result is not None or error not in report['error']:raise ValueError('contextual refusal missing')
            elif (report['failures']!=0 or result['attachment_source_to_skeleton_source']!=mapping
                  or result['root_to_skeleton_source']!=root or result['node_name_bytes']!=list(NAME)
                  or result['retail_behavior_verified'] is not False):raise ValueError(f'{name}: independent matrices differ')
        invocations.append(dict(command=command,exit_code=run.returncode,expected_error=error,receipt_sha256=None if not output.exists() else digest(output)))
    invoke('shear-reflection',request,[[0,-6,0,-25],[-6,-12,0,-33],[0,0,3,41]],[[0,-12,0,-73],[-12,-24,0,-99],[0,0,-6,71]])
    identity={**request,'attachment_parent_to_node':[[1,0,0,0],[0,1,0,0],[0,0,1,0]]}
    invoke('identity-placement',identity,[[0,6,0,-1],[-6,0,0,-15],[0,0,6,-1]],[[0,12,0,47],[-12,0,0,15],[0,0,-12,59]])
    zero={**request,'attachment_parent_to_node':[[0]*4 for _ in range(3)]}
    invoke('zero-forward-placement',zero,[[0,0,0,-1],[0,0,0,-15],[0,0,0,-1]],[[0,0,0,-1],[0,0,0,-15],[0,0,0,-1]])
    for name,patch,error in [
        ('stale-skeleton',{'expected_skeleton_sha256':[0]*32},'skeleton source SHA256 differs'),
        ('stale-attachment',{'expected_attachment_sha256':[0]*32},'attachment source SHA256 differs'),
        ('wrong-node',{'node':0},'raw node name differs'),
        ('missing-node',{'node':99},'node is not decoded'),
        ('wrong-name',{'node_name_bytes':list(b'weaponsocket')},'raw node name differs'),
        ('wrong-root',{'attachment_root':1},'not an exact footer root'),
    ]:invoke(name,{**request,**patch},error=error)
    invoke('schema', {**request,'schema_version':2},error='unsupported rigid attachment request schema',parse_error=True)
    missing=dict(request);del missing['source_policy']
    invoke('missing-policy',missing,error='missing field',parse_error=True)
    invoke('unknown-field',{**request,'default_socket':True},error='unknown field',parse_error=True)
    for name in ['skeleton','attachment','requests']:
        invoke('protected-'+name,request,error='outside every source directory',protected=inputs/name)
    if {str(path):digest(path) for path in [binary,skeleton,attachment]}!=hashes or digest(binary)!=before:raise ValueError('frozen inputs or binary changed')
    summary=dict(contract=CONTRACT,binary_sha256=before,analytic_cases=3,intended_refusals=12,invocations=invocations,frozen_hashes=hashes,retail_behavior_verified=False)
    (args.output_dir/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({key:summary[key] for key in ['contract','binary_sha256','analytic_cases','intended_refusals','retail_behavior_verified']}))
if __name__=='__main__':main()
