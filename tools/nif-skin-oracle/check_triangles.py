"""Literal authored partition conversion, source provenance and old receipts."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import check_streams as w
import check_sampled as container_writer

MAP=[3,1,1,4,0]
STRIPS=[[0,1,2,2,3,4],[4,3,1,0],[],[1],[0,1]]

def fixture(strips=True,faces=True,empty=False):
    rows=[] if empty else [[0,0,1],[4,2,3],[1,2,2]]
    chains=[[],[1],[0,1]] if empty else STRIPS
    part=w.shorts(5,9 if strips and not empty else len(rows),3,len(chains) if strips else 0,4,1,0,1)
    part+=bytes([1])+w.shorts(*MAP)+bytes([1])+w.words(*([0x80000000,0,0x3f000000,0x3fc00000]*5))
    if strips:part+=w.shorts(*(len(v) for v in chains))
    part+=bytes([int(faces)])
    if faces:part+=b''.join(w.shorts(*v) for v in (chains if strips else rows))
    part+=bytes([1])+bytes([2,1,2,0]*5)
    geometry=bytearray(w.geometry());geometry[4:6]=w.shorts(5)
    blocks=[('NiNode',w.node((2,5,6))),('NiSkinInstance',w.words(w.NULL,4,0,2,5,6)),
            ('NiTriShape',w.av()+w.words(3,1,0,w.NULL)+bytes([0])),('NiTriShapeData',bytes(geometry)),
            ('NiSkinPartition',w.words(1)+part),('NiNode',w.node()),('NiNode',w.node())]
    raw=container_writer.container(blocks)
    return raw,blocks

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()

def check(binary,prior,output):
    binary,prior=binary.resolve(),prior.resolve();output.mkdir(parents=True,exist_ok=False)
    inputs,requests=output/'inputs',output/'requests';inputs.mkdir();requests.mkdir()
    frozen={str(p):sha(p) for p in [binary,prior]};invocations=[];cases=[]
    def invoke(name,request,source,error=None,exe=binary,mode='--partition-triangles-request',flags=(),destination=None):
        p=requests/(name+'.json');p.write_text(json.dumps(request)+'\n');frozen[str(p)]=sha(p)
        destination=destination or output/(name+'.json');command=[str(exe),'nif-skin',str(source),mode,str(p),*flags,'--output',str(destination)]
        process=subprocess.run(command,capture_output=True,text=True);(output/(name+'.stderr.txt')).write_text(process.stderr)
        assert process.returncode==(2 if error=='conflict' else int(bool(error))),(name,process.stderr)
        if error in ('schema','protected','conflict'):assert not destination.exists();value=None
        else:
            report=json.loads(destination.read_text());value=report['evaluation'];assert report['failures']==int(bool(error))
            if error:assert value is None and report['error'] and (error is True or error in report['error']),report
            else:assert report['error'] is None
        invocations.append(dict(case=name,command=command,exit_code=process.returncode,expected_error=error,receipt_sha256=sha(destination) if destination.exists() else None))
        return value,destination
    for name,strips,empty in [('strips',True,False),('triangles',False,False),('short-strips',True,True),('empty-triangles',False,True)]:
        source=inputs/(name+'.nif');raw,blocks=fixture(strips=strips,empty=empty);source.write_bytes(raw);frozen[str(source)]=sha(source)
        base=dict(schema_version=1,expected_source_sha256=list(bytes.fromhex(sha(source))),geometry=2,partition_block=4,partition_ordinal=0,
                  policy=dict(kind='alternating_strip_winding_skip_repeated_index_v1'))
        packet,_=invoke(name,base,source)
        if empty:expected=[]
        elif strips:expected=[(0,0,0,[0,1,2],[3,1,1]),(3,0,3,[3,2,4],[4,1,0]),(4,1,0,[4,3,1],[0,4,1]),(5,1,1,[1,3,0],[1,4,3])]
        else:expected=[(0,None,0,[0,0,1],[3,3,1]),(1,None,1,[4,2,3],[0,1,4]),(2,None,2,[1,2,2],[1,1,1])]
        assert packet['triangles']==[dict(source_topology_kind='authored_strips' if strips else 'authored_triangles',
            source_primitive_ordinal=ordinal,strip_ordinal=strip,primitive_step=step,local_vertices=local,source_vertices=mapped)
            for ordinal,strip,step,local,mapped in expected]
        u=packet['usage'];assert [u[k] for k in ['raw_primitives','omitted_connectors','generated_triangles','draw_indices']]==([0,0,0,0] if empty else [6,2,4,12] if strips else [3,0,3,9])
        assert packet['source_faces_present']==1 and packet['declared_source_triangles']==(0 if empty else 9 if strips else 3)
        assert u['source_decodes']==u['source_sha256_traversals']==0 and u['combined_retained_bytes']==u['streams_retained_bytes']+u['retained_bytes']
        assert not packet['retail_behavior_verified']
        for field,block in [('geometry',2),('geometry_data',3),('instance',1),('partition',4)]:
            span=packet['identity'][field];assert raw[span['offset']:span['offset']+span['bytes']]==blocks[block][1]
            assert span['block']==block and span['sha256']==hashlib.sha256(blocks[block][1]).hexdigest()
        oldrequest={k:v for k,v in base.items() if k!='policy'};receipts=[]
        for label,exe in [('old',prior),('new',binary)]:
            streams,path=invoke(name+'-streams-'+label,oldrequest,source,exe=exe,mode='--partition-streams-request')
            receipts.append(path.read_bytes());assert streams['identity']==packet['identity']
        assert receipts[0]==receipts[1];cases.append((source,base))
    source,base=cases[0]
    stale=copy.deepcopy(base);stale['expected_source_sha256'][0]^=1;invoke('stale',stale,source,error='SHA256 differs')
    for name,request in [('owner',dict(base,geometry=0)),('partition',dict(base,partition_block=5)),('ordinal',dict(base,partition_ordinal=1))]:
        invoke(name,request,source,error=True)
    absent=inputs/'absent-faces.nif';absent.write_bytes(fixture(faces=False)[0]);frozen[str(absent)]=sha(absent)
    invoke('absent-faces',dict(base,expected_source_sha256=list(bytes.fromhex(sha(absent)))),absent,error=True)
    for name,request in [('version',dict(base,schema_version=2)),('unknown',dict(base,extra=True)),
        ('negative',dict(base,partition_ordinal=-1)),('policy-extra',dict(base,policy=dict(kind=base['policy']['kind'],flip=True))),
        ('policy',dict(base,policy=dict(kind='alternating_winding'))),('short-sha',dict(base,expected_source_sha256=[0]*31))]:
        invoke(name,request,source,error='schema')
    for flag in ['--geometry-streams-request','--palette-packet-request','--partition-streams-request']:
        invoke('conflict-'+flag[2:],base.copy(),source,error='conflict',flags=[flag,str(requests/'stale.json')])
    for name,folder in [('source',inputs),('request',requests)]:invoke('protected-'+name,base.copy(),source,error='protected',destination=folder/'blocked.json')
    for p,digest in frozen.items():assert sha(Path(p))==digest
    result=dict(contract='engineering-partition-triangle-proof-v1',literal_cases=4,complete_old_stream_reports_byte_equal=4,
                intended_refusals=sum(bool(v.get('expected_error')) for v in invocations),invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True,type=Path);p.add_argument('--previous-binary',required=True,type=Path);p.add_argument('--output',required=True,type=Path)
    a=p.parse_args();r=check(a.binary,a.previous_binary,a.output);print(json.dumps({k:v for k,v in r.items() if k not in ('invocations','frozen_hashes')}))
