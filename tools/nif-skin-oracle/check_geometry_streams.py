"""Independent whole-geometry source words, CSR and literal CPU positions."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import check_sampled as w

POSITIONS = [[2,-0.0,1],[-2,3,0],[4,-1,-2],[0,2,3]]
NORMALS = [[0,-0.0,1],[1,0,0],[0,1,-0.0],[1,2,3]]
UV = [[-0.0,0],[.25,-0.0],[1,2],[-1,.5]]
WEIGHTS = [[(0,.25),(0,.125),(0,-0.0),(0,0.0),(0,.125),(1,1),(3,1)],
           [(0,.5),(0,.25),(2,1)]]

def fixture(present=True, strips=True, additional=False, second_uv=False):
    def geometry(instance):
        return w.av(w.ID,[777,888,999],1)+w.words(4,instance,0,w.NULL)+bytes([0])
    def skin(translation):
        out=w.transform(w.ID,translation,1)+w.words(2)+bytes([1])
        for terms in WEIGHTS:
            out+=w.transform(w.ID,[0,0,0],1)+w.floats(0,0,0,11)+w.shorts(len(terms))
            for vertex,weight in terms:out+=w.shorts(vertex)+w.floats(weight)
        return out
    mesh=w.words(0xfffffff9)+w.shorts(4)+bytes([9,6,1])+b''.join(w.floats(*v) for v in POSITIONS)
    mesh+=w.shorts(0x1043 if present else 2)+bytes([int(present)])
    if present:
        mesh+=b''.join(w.floats(*v) for v in NORMALS)
        mesh+=w.floats(1,-0.0,0)*4+w.floats(-0.0,1,0)*4
    mesh+=w.floats(-0.0,3,4,11)+bytes([int(present)])
    if present:
        mesh+=w.floats(-0.0,.25,1.5,.875)*4
        uv=b''.join(w.floats(*v) for v in UV);mesh+=uv
        if second_uv:mesh+=uv
    mesh+=w.shorts(0x4000)+w.words(10 if additional else w.NULL)
    if strips:mesh+=w.shorts(7,2,8,3)+bytes([1])+w.shorts(0,1,2,2,1,1,2,3,3,2,0)
    else:mesh+=w.shorts(2)+w.words(6)+bytes([1])+w.shorts(0,1,2,3,2,0,1,3,2,2,0)
    geometry_type='NiTriStrips' if strips else 'NiTriShape';data_type=geometry_type+'Data'
    blocks=[('NiNode',w.node(w.ID,[10,20,30],1,[1,2,3,7])),
            ('NiNode',w.node(w.ID,[3,0,0],1,[])),('NiNode',w.node(w.ID,[0,5,0],1,[])),
            (geometry_type,geometry(5)),(data_type,mesh),('NiSkinInstance',w.words(6,w.NULL,0,2,1,2)),
            ('NiSkinData',skin([0,0,0])),(geometry_type,geometry(8)),('NiSkinInstance',w.words(9,w.NULL,0,2,2,1)),
            ('NiSkinData',skin([1,0,0]))]
    if additional:blocks.append(('NiAdditionalGeometryData',w.words(0x12345678,0xabcdef01)))
    return w.container(blocks),blocks

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def bits(rows):return [[struct.unpack('<I',w.floats(v))[0] for v in row] for row in rows]

def check(binary,prior,output):
    binary,prior=binary.resolve(),prior.resolve();output.mkdir(parents=True,exist_ok=False)
    inputs,requests=output/'inputs',output/'requests';inputs.mkdir();requests.mkdir()
    frozen={str(p):sha(p) for p in [binary,prior]};invocations=[]
    def invoke(name,request,source,error=None,executable=binary,mode='--geometry-streams-request',flags=(),destination=None,exit_code=None):
        path=requests/(name+'.json');path.write_text(json.dumps(request)+'\n');frozen[str(path)]=sha(path)
        destination=destination or output/(name+'.json')
        command=[str(executable),'nif-skin',str(source),mode,str(path),*flags,'--output',str(destination)]
        process=subprocess.run(command,capture_output=True,text=True);(output/(name+'.stderr.txt')).write_text(process.stderr)
        expected=exit_code if exit_code is not None else int(bool(error));assert process.returncode==expected,(name,process.stderr)
        if error in ('schema','protected','conflict'):
            assert not destination.exists();value=None
        else:
            report=json.loads(destination.read_text());assert report['failures']==expected
            value=report['evaluation']
            if error:assert value is None and report['error'] and (error is True or error in report['error']),report
            else:assert report['error'] is None
        invocations.append(dict(case=name,command=command,exit_code=process.returncode,expected_error=error,
                                receipt_sha256=sha(destination) if destination.exists() else None))
        return value,destination
    cases=[]
    for name,present,strips,additional in [('present-strips',True,True,False),('present-triangles',True,False,False),
                                         ('absent',False,False,False),('additional',True,False,True)]:
        source=inputs/(name+'.nif');data,blocks=fixture(present,strips,additional);source.write_bytes(data);frozen[str(source)]=sha(source)
        base=dict(schema_version=1,expected_source_sha256=list(bytes.fromhex(sha(source))),geometry=3,instance=5,
                  weights=dict(kind='preserve_raw_nonnegative'))
        value,_=invoke(name,base,source);packet,pose=value['streams'],value['stored_pose']
        assert packet['attributes']==dict(positions_bits=bits(POSITIONS),normals_bits=bits(NORMALS) if present else [],
              tangents_bits=bits([[1,-0.0,0]]*4) if present else [],bitangents_bits=bits([[-0.0,1,0]]*4) if present else [],
              colors_bits=bits([[-0.0,.25,1.5,.875]]*4) if present else [],uv_sets_bits=[bits(UV)] if present else [])
        assert packet['presence']==dict(vertices=True,normals=present,colors=present,tangents=present,uv_sets=int(present))
        assert packet['metadata']==dict(group_id=-7,vertex_count=4,keep_flags=9,compress_flags=6,data_flags=0x1043 if present else 2,
              bound_bits=bits([[-0.0,3,4,11]])[0],consistency_flags=0x4000,declared_triangles=7 if strips else 2,
              source_triangle_count_matches=True,strip_degenerate_triangles=4 if strips else 0)
        assert packet['influences']['vertex_offsets']==[0,7,8,9,10]
        expected_entries=[]
        for vertex in range(4):
            for bone,terms in enumerate(WEIGHTS):
                expected_entries.extend(dict(bone_ordinal=bone,weight_bits=struct.unpack('<I',w.floats(weight))[0],
                                             source_weight_ordinal=ordinal) for ordinal,(v,weight) in enumerate(terms) if v==vertex)
        assert packet['influences']['entries']==expected_entries
        assert packet['vertices']==[dict(source_vertex=i,influence_start=start,influence_count=count)
                                    for i,start,count in [(0,0,7),(1,7,1),(2,8,1),(3,9,1)]]
        if strips:
            assert packet['topology']==dict(kind='strips',lengths=[8,3],present=True,indices=[[0,1,2,2,1,1,2,3],[3,2,0]])
            expected_triangles=[([0,1,2],0,0,0),([1,3,2],5,0,5),([3,2,0],6,1,0)]
        else:
            assert packet['topology']==dict(kind='triangles',declared_points=6,present=True,indices=[[0,1,2],[3,2,0]],match_groups=[[2,2,0]])
            expected_triangles=[([0,1,2],0,None,None),([3,2,0],1,None,None)]
        assert packet['triangles']==[dict(vertices=v,source_primitive_ordinal=p,strip_ordinal=s,strip_step_ordinal=t) for v,p,s,t in expected_triangles]
        for field,block in [('geometry',3),('geometry_data',4),('instance',5),('skin_data',6)]:
            span=packet['identity'][field];assert span['block']==block and span['bytes']==len(blocks[block][1])
            assert span['sha256']==hashlib.sha256(blocks[block][1]).hexdigest()
            assert data[span['offset']:span['offset']+span['bytes']]==blocks[block][1]
        assert packet['identity']['source_sha256']==sha(source)
        assert not packet['faithful_renderer_ready'] and not packet['retail_behavior_verified'] and 'authority' not in packet
        assert pose['skin']['positions']==[[4,3.75,1.25],[1,3,0],[4,4,-2],[3,2,3]]
        assert pose['skin']['normals']==([[0,0,1.25],[1,0,0],[0,1,0],[1,2,3]] if present else [])
        assert pose['skin']['palette'][0]['matrix']==[[1,0,0,3],[0,1,0,0],[0,0,1,0]]
        assert pose['skin']['palette'][1]['matrix']==[[1,0,0,0],[0,1,0,5],[0,0,1,0]]
        assert pose['skin']['skin_to_source_world']==[[1,0,0,10],[0,1,0,20],[0,0,1,30]]
        assert pose['skin']['weight_sums']==[1.25,1,1,1]
        assert pose['usage']['binding_decodes']==pose['usage']['scene_decodes']==pose['usage']['full_source_sha256_traversals']==pose['usage']['csr_builds']==0
        assert pose['usage']['packet_retained_bytes']==packet['usage']['retained_bytes']
        assert pose['usage']['combined_retained_bytes']==packet['usage']['retained_bytes']+pose['usage']['charged_output_bytes']
        assert packet['usage']['binding_decodes']==packet['usage']['scene_decodes']==packet['usage']['full_source_sha256_traversals']==packet['usage']['csr_builds']==1
        if additional:
            extra=packet['additional_geometry_data'];assert extra['link']==extra['span']['block']==10 and not extra['payload_decoded']
            assert extra['block_type']=='NiAdditionalGeometryData' and extra['span']['sha256']==hashlib.sha256(blocks[10][1]).hexdigest()
        else:assert packet['additional_geometry_data'] is None
        cases.append((source,base,value))
    return finish_check(binary,prior,output,inputs,requests,frozen,invocations,invoke,cases)

def finish_check(binary,prior,output,inputs,requests,frozen,invocations,invoke,cases):
    old_equal=[]
    for i,(source,base,new) in enumerate(cases):
        request=dict(schema_version=1,expected_sha256=base['expected_source_sha256'],geometry=3,weights=base['weights'])
        receipts=[]
        for label,exe in [('prior',prior),('new',binary)]:
            cold,path=invoke(f'cold-{i}-{label}',request,source,executable=exe,mode='--influences-request')
            receipts.append(path.read_bytes())
            assert cold['table']==new['streams']['influences']
            assert cold['pose']==new['stored_pose']['skin']
        assert receipts[0]==receipts[1];old_equal.append(str(source))
    source,base,_=cases[0]
    second,_=invoke('shared-data-other-instance',dict(base,geometry=7,instance=8),source)
    assert [second['streams']['identity'][name]['block'] for name in ['geometry','geometry_data','instance','skin_data']]==[7,4,8,9]
    assert second['stored_pose']['skin']['positions']==[[6,2.5,1.25],[-1,8,0],[8,-1,-2],[1,7,3]]
    assert second['stored_pose']['skin']['skin_to_source_world']==[[1,0,0,9],[0,1,0,20],[0,0,1,30]]
    invoke('same-data-wrong-instance',dict(base,geometry=7),source,error='skin instance differs')
    invoke('other-instance',dict(base,instance=8),source,error='skin instance differs')
    stale=copy.deepcopy(base);stale['expected_source_sha256'][0]^=1;invoke('stale',stale,source,error='SHA256 differs')
    invoke('geometry-not-owner',dict(base,geometry=0),source,error=True)
    invoke('late-nonunit',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=0)),source,error='raw weight sum')
    invoke('invalid-tolerance',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=2)),source,error='weight tolerance')
    malformed=inputs/'malformed-second-uv.nif';malformed.write_bytes(fixture(second_uv=True)[0]);frozen[str(malformed)]=sha(malformed)
    invoke('malformed-second-uv',dict(base,expected_source_sha256=list(bytes.fromhex(sha(malformed)))),malformed,error=True)
    changed=inputs/'changed-same-ids.nif';raw,blocks=fixture();blocks[3]=(blocks[3][0],w.av(w.ID,[778,888,999],1)+w.words(4,5,0,w.NULL)+bytes([0]))
    changed.write_bytes(w.container(blocks));frozen[str(changed)]=sha(changed)
    invoke('changed-source-same-blocks',base.copy(),changed,error='SHA256 differs')
    for name,request in [
        ('schema-version',dict(base,schema_version=2)),('unknown',dict(base,extra=True)),
        ('short-sha',dict(base,expected_source_sha256=base['expected_source_sha256'][:-1])),
        ('negative-geometry',dict(base,geometry=-1)),('negative-instance',dict(base,instance=-1)),
        ('unknown-raw',dict(base,weights=dict(kind='preserve_raw_nonnegative',repair=True))),
        ('unknown-unit',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=0,repair=True))),
        ('unknown-policy',dict(base,weights=dict(kind='normalize'))),
        ('nan-tolerance',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=float('nan')))),
    ]:invoke(name,request,source,error='schema')
    for flag in ['--include-bindings','--partition-streams-request','--partition-triangles-request','--palette-packet-request','--shared-skin-job-request']:
        extra=[flag] if flag=='--include-bindings' else [flag,str(requests/'stale.json')]
        invoke('conflict-'+flag[2:],base.copy(),source,error='conflict',flags=extra,exit_code=2)
    for name,folder in [('source',inputs),('request',requests)]:
        invoke('protected-'+name,base.copy(),source,error='protected',destination=folder/'forbidden.json')
    occupied=output/'occupied.json';occupied.write_bytes(b'preserved output\n');frozen[str(occupied)]=sha(occupied)
    command=[str(binary),'nif-skin',str(source),'--geometry-streams-request',str(requests/'present-strips.json'),'--output',str(occupied)]
    process=subprocess.run(command,capture_output=True,text=True);assert process.returncode==1
    invocations.append(dict(case='occupied-output',command=command,exit_code=1,expected_error='preexisting',receipt_sha256=sha(occupied)))
    for path,digest in frozen.items():assert sha(Path(path))==digest,(path,digest)
    result=dict(contract='engineering-whole-geometry-stream-proof-v1',literal_cases=5,
                old_complete_cold_reports_byte_equal=old_equal,intended_refusals=sum(bool(v.get('expected_error')) for v in invocations),
                invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True,type=Path);parser.add_argument('--previous-binary',required=True,type=Path)
    parser.add_argument('--output',required=True,type=Path);args=parser.parse_args()
    result=check(args.binary,args.previous_binary,args.output)
    print(json.dumps({k:v for k,v in result.items() if k not in ('invocations','frozen_hashes')}))
