"""Independent noncommuting source and explicit f32 rounding through the CLI."""
import argparse
import copy
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import check_sampled as w

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def bits(matrix):return [[struct.unpack('<I',w.floats(value))[0] for value in row] for row in matrix]
def skin():
    out=w.transform(w.ID,[1,-2,3],.5)+w.words(2)+bytes([1])
    for r,t,s,terms in [(w.ID,[-1,0,2],1,[(0,.25),(0,.5),(1,1)]),
                        (w.R90,[0,-1,0],2,[(0,.75),(2,1)])]:
        out+=w.transform(r,t,s)+w.floats(0,0,0,11)+w.shorts(len(terms))
        for vertex,weight in terms:out+=w.shorts(vertex)+w.floats(weight)
    return out

def check(binary,prior,output):
    binary,prior=binary.resolve(),prior.resolve();output.mkdir(parents=True,exist_ok=False)
    inputs,requests=output/'inputs',output/'requests';inputs.mkdir();requests.mkdir()
    frozen={str(p):sha(p) for p in [binary,prior]};invocations=[];old_equal=[]
    def source(name,blocks):
        p=inputs/(name+'.nif');p.write_bytes(w.container(blocks));frozen[str(p)]=sha(p);return p
    def request(p,tolerance=0):
        return dict(schema_version=1,expected_source_sha256=list(bytes.fromhex(sha(p))),geometry=3,
                    weights=dict(kind='preserve_raw_nonnegative'),precision=dict(kind='finite_nearest_f32',maximum_absolute_error=tolerance))
    def invoke(name,value,p,error=None,exe=binary,mode='--palette-packet-request',flags=(),destination=None):
        r=requests/(name+'.json');r.write_text(json.dumps(value)+'\n');frozen[str(r)]=sha(r)
        destination=destination or output/(name+'.json');command=[str(exe),'nif-skin',str(p),mode,str(r),*flags,'--output',str(destination)]
        process=subprocess.run(command,capture_output=True,text=True);(output/(name+'.stderr.txt')).write_text(process.stderr)
        assert process.returncode==(2 if error=='conflict' else int(bool(error))),(name,process.stderr)
        if error in ('schema','protected','conflict'):assert not destination.exists();packet=None
        else:
            report=json.loads(destination.read_text());packet=report['evaluation'];assert report['failures']==int(bool(error))
            if error:assert packet is None and report['error'] and (error is True or error in report['error']),report
            else:assert report['error'] is None
        invocations.append(dict(case=name,command=command,exit_code=process.returncode,expected_error=error,receipt_sha256=sha(destination) if destination.exists() else None))
        return packet,destination
    blocks=w.fixture();blocks[5]=('NiSkinData',skin());p=source('noncommuting-nonunit',blocks);base=request(p)
    packet,_=invoke('noncommuting',base,p)
    matrices=[[[0,1,0,2.5],[-1,0,0,-.5],[0,0,1,5]],[[0,-3,0,0],[3,0,0,-1.5],[0,0,3,3.5]]]
    assert packet['palette']==[dict(ordinal=i,node=i+1,matrix=dict(row_major_3x4_bits=bits(matrix),maximum_absolute_error_bound=0)) for i,matrix in enumerate(matrices)]
    assert packet['skin_to_source_world']==dict(row_major_3x4_bits=bits([[0,-4,0,-4],[4,0,0,-6],[0,0,4,-4]]),maximum_absolute_error_bound=0)
    assert [packet[k] for k in ['geometry','geometry_data','instance','skin_data','skeleton_root']]==[3,6,4,5,0]
    assert packet['source_sha256']==sha(p) and packet['weights']==base['weights'] and not packet['retail_behavior_verified']
    assert packet['usage']['scalar_conversions']==36 and packet['usage']['source_decodes']==packet['usage']['full_source_sha256_traversals']==1
    # Complete old cold-CSR receipt bytes and all original f64 matrix fields.
    cold=dict(schema_version=1,expected_sha256=base['expected_source_sha256'],geometry=3,weights=base['weights']);receipts=[]
    for label,exe in [('old',prior),('new',binary)]:
        value,path=invoke('cold-'+label,cold,p,exe=exe,mode='--influences-request');receipts.append(path.read_bytes())
        assert [b['matrix'] for b in value['pose']['palette']]==matrices
        assert value['pose']['weight_sums']==[1.5,1,1]
        assert packet['usage']['deformation_charged_bytes']==value['pose']['retained_bytes']
        # The old CSR evaluator has table validation overhead; transport uses the
        # original cold deformer. Its full receipt remains byte-identical above.
    assert receipts[0]==receipts[1];old_equal.append('noncommuting-nonunit')
    # Independent halfway composition rounds the odd lower mantissa upward to
    # the next even mantissa. Every matrix word is literal, including off-diagonal.
    half=2**-24;blocks=w.fixture();blocks[0]=('NiNode',w.node(w.ID,[0,0,0],1,[1,2,3]));blocks[1]=('NiNode',w.node([[1,3*half,0],[0,1,0],[0,0,1]],[0,0,0],1,[]))
    blocks[2]=('NiNode',w.node(w.ID,[0,0,0],1,[]));data=w.transform(w.ID,[0,0,0],1)+w.words(2)+bytes([1])
    for rotation,terms in [([[1,0,0],[1,1,0],[0,0,1]],[(0,.25),(1,1)]),(w.ID,[(0,.75),(2,1)])]:
        data+=w.transform(rotation,[0,0,0],1)+w.floats(0,0,0,11)+w.shorts(len(terms))
        for vertex,weight in terms:data+=w.shorts(vertex)+w.floats(weight)
    blocks[5]=('NiSkinData',data);halfsource=source('halfway-upper-even',blocks)
    rounded,_=invoke('halfway',request(halfsource,half),halfsource)
    assert rounded['palette'][0]['matrix']==dict(row_major_3x4_bits=[[0x3f800002,0x34400000,0,0],[0x3f800000,0x3f800000,0,0],[0,0,0x3f800000,0]],maximum_absolute_error_bound=half)
    assert rounded['palette'][1]['matrix']==dict(row_major_3x4_bits=[[0x3f800000,0,0,0],[0,0x3f800000,0,0],[0,0,0x3f800000,0]],maximum_absolute_error_bound=0)
    assert rounded['skin_to_source_world']==dict(row_major_3x4_bits=[[0x3f800000,0,0,0],[0,0x3f800000,0,0],[0,0,0x3f800000,0]],maximum_absolute_error_bound=0)
    invoke('halfway-under',request(halfsource,math.nextafter(half,0)),halfsource,error='precision tolerance exceeded')
    blocks=w.fixture();blocks[0]=('NiNode',w.node(w.ID,[0,0,0],struct.unpack('<f',struct.pack('<I',0x7f7fffff))[0],[1,2,3]));overflow=source('late-placement-overflow',blocks)
    invoke('late-placement-overflow',request(overflow,float.fromhex('0x1.fffffffffffffp+1023')),overflow,error='finite f32 range')
    stale=copy.deepcopy(base);stale['expected_source_sha256'][0]^=1;invoke('stale',stale,p,error='SHA256 differs')
    invoke('geometry',dict(base,geometry=0),p,error=True)
    invoke('nonunit-refusal',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=0)),p,error='raw weight sum')
    invoke('weight-tolerance',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=2)),p,error='weight tolerance')
    invoke('negative-precision',dict(base,precision=dict(kind='finite_nearest_f32',maximum_absolute_error=-1)),p,error='finite and nonnegative')
    for name,value in [('version',dict(base,schema_version=2)),('unknown',dict(base,extra=True)),
        ('precision-extra',dict(base,precision=dict(kind='finite_nearest_f32',maximum_absolute_error=0,transpose=True))),
        ('precision-kind',dict(base,precision=dict(kind='nearest'))),
        ('raw-extra',dict(base,weights=dict(kind='preserve_raw_nonnegative',normalize=True))),
        ('unit-extra',dict(base,weights=dict(kind='require_unit_sum',absolute_tolerance=0,normalize=True))),
        ('nan',dict(base,precision=dict(kind='finite_nearest_f32',maximum_absolute_error=float('nan')))),
        ('negative-geometry',dict(base,geometry=-1))]:invoke(name,value,p,error='schema')
    for flag in ['--geometry-streams-request','--partition-triangles-request','--prepared-influences-request']:
        invoke('conflict-'+flag[2:],base.copy(),p,error='conflict',flags=[flag,str(requests/'stale.json')])
    for name,folder in [('source',inputs),('request',requests)]:invoke('protected-'+name,base.copy(),p,error='protected',destination=folder/'blocked.json')
    for path,digest in frozen.items():assert sha(Path(path))==digest
    result=dict(contract='engineering-finite-f32-palette-proof-v1',literal_cases=2,complete_old_cold_reports_byte_equal=old_equal,
                intended_refusals=sum(bool(v.get('expected_error')) for v in invocations),invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n');return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True,type=Path);p.add_argument('--previous-binary',required=True,type=Path);p.add_argument('--output',required=True,type=Path)
    a=p.parse_args();r=check(a.binary,a.previous_binary,a.output);print(json.dumps({k:v for k,v in r.items() if k not in ('invocations','frozen_hashes')}))
