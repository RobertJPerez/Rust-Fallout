"""Independent ancestry/held-value literals and full old local receipt checks.

Packet construction and suffix span arithmetic only; no decoder or sampler.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import check_clip as source

NULL=2**32-1
CONTRACT='engineering-required-path-local-visibility-v1'
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def controller(target,interpolator,chain=NULL,flags=0x4C):
    return source.words(chain)+struct.pack('<H',flags)+source.floats(13,-7,100,101)+source.words(target,interpolator)
def group(times,values):
    return source.words(len(times))+(source.words(5) if times else b'')+b''.join(source.floats(t)+bytes([v]) for t,v in zip(times,values))
def node(controller,flags,children):
    return source.words(0,0,controller,flags)+source.floats(5,-6,7,1,0,0,0,1,0,0,0,1,2)+source.words(0,NULL,len(children),*children,0)
def fixture(leaf_chain=NULL,leaf_target=0,leaf_flags=0x4C,leaf_type='NiVisController',timeline=False,mid_pose=0,orphan=False,unresolved=False,leaf_values=(1,0,1)):
    packets=[node(1,0x102030FE,[]),controller(leaf_target,2,leaf_chain,leaf_flags),bytes([2])+source.words(3),group((-2,1,8),leaf_values),
        node(NULL,0xCAFE0001,[] if orphan else [0]),node(6,0x90000002,[4]),controller(5,7),bytes([mid_pose])+source.words(NULL),source.words(0),
        node(10,0xABCDEF01,[5,14 if unresolved else 13]),controller(9,11),bytes([2])+source.words(12),group((-2,1,8),(0,1,0)),
        node(14,0xDDEEFF01,[]),controller(13,NULL,14,0x6C)]
    names=['NiNode','NiVisController','NiBoolInterpolator','NiBoolData']
    if leaf_type!='NiVisController':names.append(leaf_type)
    if timeline:names.append('NiBoolTimelineInterpolator')
    types=[0,names.index(leaf_type),names.index('NiBoolTimelineInterpolator') if timeline else 2,3,0,0,1,2,3,0,1,2,3,0,1]
    data=source.container(names,packets,types,[b'RawName\x00\xff']);data=data[:-4]+source.words(9)
    return data,packets
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',required=True,type=Path)
    parser.add_argument('--prior-binary',required=True,type=Path);parser.add_argument('--output-dir',required=True,type=Path);args=parser.parse_args()
    args.output_dir.mkdir(exist_ok=False);inputs=args.output_dir/'inputs';inputs.mkdir();requests=inputs/'requests';requests.mkdir()
    binary=args.binary.resolve();prior=args.prior_binary.resolve();frozen={str(p):sha(p) for p in [binary,prior]}
    data,packets=fixture();path=inputs/'path.nif';path.write_bytes(data);frozen[str(path)]=sha(path);commands=[];equal=[]
    def channels(time):return [dict(object=0,controller=1,source_time=time),dict(object=9,controller=10,source_time=time),dict(object=5,controller=6,source_time=-0.0)]
    def invoke(name,time=0,patch=None,input_path=path,error=None):
        req=dict(schema_version=1,expected_source_sha256=list(bytes.fromhex(sha(input_path))),object=0,channels=channels(time));req.update(patch or {})
        request=requests/(name+'.json');request.write_text(json.dumps(req)+'\n');dest=args.output_dir/(name+'.json')
        command=[str(binary),'nif-visibility-path',str(input_path),'--request',str(request),'--output',str(dest)]
        run=subprocess.run(command,capture_output=True,text=True);(args.output_dir/(name+'.stderr.txt')).write_text(run.stderr)
        if run.returncode!=(1 if error else 0):raise ValueError(f'{name}: exit{run.returncode}: {run.stderr}')
        report=json.loads(dest.read_text())
        if report['contract']!=CONTRACT or report['sha256']!=sha(input_path):raise ValueError('exact source identity differs')
        if error:
            if report['failures']!=1 or report['evaluation'] is not None or error not in report['error']:raise ValueError(f'{name}: atomic contextual refusal missing')
        elif report['failures'] or report['evaluation']['retail_behavior_verified'] is not False:raise ValueError('engineering-only observation missing')
        commands.append(dict(command=command,exit_code=run.returncode,expected_error=error,receipt_sha256=sha(dest),request_sha256=sha(request)))
        return report['evaluation'],req
    cases=[('first',-2,[0,0,None,1],0),('before-middle',1-2**-40,[0,0,None,1],0),
        ('middle',1,[1,0,None,0],1),('after-middle',1+2**-40,[1,0,None,0],1),
        ('held-middle',7,[1,0,None,0],1),('last',8,[0,0,None,1],2)]
    # Output order is root9, intermediate5, static4, leaf0. Each literal row
    # remains independent; stored flags are never combined with held values.
    for name,time,expected_values,key_index in cases:
        result,req=invoke(name,time);reverse,_=invoke(name+'-permuted',time,{'channels':channels(time)[::-1]})
        if result!=reverse:raise ValueError('caller order changed required path observations')
        nodes=result['nodes']
        if [node['object']['block'] for node in nodes]!=[9,5,4,0] or [node['parent'] for node in nodes]!=[None,9,5,4]:raise ValueError('literal higher-ID hierarchy differs')
        if [node['object_flags'] for node in nodes]!=[0xABCDEF01,0x90000002,0xCAFE0001,0x102030FE]:raise ValueError('raw flag words changed')
        if [node['local']['raw_value'] if node['local'] else None for node in nodes]!=expected_values:raise ValueError('literal per-node values differ')
        if result['boolean_source_decodes']!=1 or result['scene_decodes']!=1 or 'effective_visible' in result or 'visible' in result:raise ValueError('source-local policy changed')
        for node in nodes:
            spans=[node['object']]
            if node['local']:
                local=node['local'];wanted=next(r for r in req['channels'] if r['object']==node['object']['block'])
                if local['requested_time_f64_bits']!=struct.unpack('<Q',struct.pack('<d',wanted['source_time']))[0]:raise ValueError('binary64 explicit time word differs')
                selection={'kind':'authored_pose'} if node['object']['block']==5 else {'kind':'held_key','index':key_index,'time_bits':struct.unpack('<I',source.floats((-2,1,8)[key_index]))[0]}
                if local['selection']!=selection:raise ValueError('literal held ordinal/time word differs')
                if local['unapplied_controller_fields']['frequency_bits']!=struct.unpack('<I',source.floats(13))[0] or local['unapplied_controller_fields']['start_bits']!=struct.unpack('<I',source.floats(100))[0]:raise ValueError('unapplied source clocks changed')
                spans.extend(local[k] for k in ['object','controller','interpolator']);spans.extend([local['data']] if local['data'] else [])
                outputs=[]
                for label,exe in [('prior',prior),('current',binary)]:
                    dest=args.output_dir/(name+'-'+str(wanted['object'])+'-'+label+'.json')
                    command=[str(exe),'nif-source-pose',str(path),'--local-visibility','--object',str(wanted['object']),'--controller',str(wanted['controller']),'--source-time',str(wanted['source_time']),'--output',str(dest)]
                    run=subprocess.run(command,capture_output=True,text=True)
                    if run.returncode:raise ValueError(run.stderr)
                    outputs.append(dest);commands.append(dict(command=command,exit_code=run.returncode,receipt_sha256=sha(dest)))
                if outputs[0].read_bytes()!=outputs[1].read_bytes() or json.loads(outputs[0].read_text())['evaluation']!=local:raise ValueError('complete old source-local observation changed')
                equal.append(sha(outputs[0]))
            for span in spans:
                block=span['block'];offset=len(data)-8-sum(len(p) for p in packets[block:]);payload=packets[block]
                if span['offset']!=offset or span['bytes']!=len(payload) or span['sha256']!=hashlib.sha256(payload).hexdigest():raise ValueError('independent authored span differs')
    for name,patch,error in [('stale',{'expected_source_sha256':[0]*32},'source SHA256 differs'),('absent-object',{'object':99},'ancestry is unavailable'),
        ('missing',{'channels':channels(0)[:2]},'no explicit channel'),('duplicate',{'channels':[channels(0)[0]]*2},'duplicate explicit channel'),
        ('wrong-controller',{'channels':[{**r,'controller':10} if r['object']==0 else r for r in channels(0)]},'object.controller differs'),
        ('unrelated',{'channels':channels(0)+[dict(object=13,controller=14,source_time=0)]},'outside required ancestry path'),
        ('static-channel',{'channels':channels(0)+[dict(object=4,controller=6,source_time=0)]},'without a controller')]:invoke(name,patch=patch,error=error)
    for name,time in [('late-before',-3),('late-after',9)]:
        late=channels(0);late[0]['source_time']=time;invoke(name,patch={'channels':late},error='extrapolate')
    variants=[('chain',dict(leaf_chain=1),'controller chain'),('wrong-target',dict(leaf_target=NULL),'controller.target differs'),
        ('manager',dict(leaf_flags=0x6C),'flags require unavailable semantics'),('unknown-controller',dict(leaf_type='NiTransformController'),'not supported NiVisController'),
        ('timeline',dict(timeline=True),'timeline key crossing'),('bad-pose',dict(mid_pose=2),'pose Boolean value unavailable'),
        ('invalid-key',dict(leaf_values=(1,2,1)),'key Boolean value unavailable'),('orphan',dict(orphan=True),'not reachable from footer'),
        ('unresolved',dict(unresolved=True),'unresolved scene ancestry')]
    for name,options,error in variants:
        changed,_=fixture(**options);source_path=inputs/(name+'.nif');source_path.write_bytes(changed);frozen[str(source_path)]=sha(source_path);invoke(name,input_path=source_path,error=error)
    for file,digest in frozen.items():
        if sha(Path(file))!=digest:raise ValueError('frozen source/binary changed')
    summary=dict(contract=CONTRACT,literal_paths=len(cases),per_path_nodes=4,permutation_cases=len(cases),prior_complete_local_reports_byte_equal=equal,
        intended_refusals=18,invocations=commands,frozen_hashes=frozen,retail_behavior_verified=False)
    (args.output_dir/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({k:v for k,v in summary.items() if k not in ('invocations','frozen_hashes','prior_complete_local_reports_byte_equal')}))
if __name__=='__main__':main()
