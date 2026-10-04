"""Literal physical-source expectations through a frozen marker-query CLI."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import fixtures

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def check(binary, output, original=None, native=None):
    binary = binary.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inputs = output / 'inputs'
    inputs.mkdir()
    requests = output / 'requests'
    requests.mkdir()
    blocks, strings = fixtures.fixture(34, 'ordinary')
    source = inputs / 'authored.kf'
    source.write_bytes(fixtures.container(34, blocks, strings))
    frozen = {str(path): sha(path) for path in [binary, source]}
    base = dict(schema_version=1, expected_sha256=list(bytes.fromhex(sha(source))),
                sequence=1, source_start=0, source_end=2)
    invocations = []
    def run(name, request, source_path=source, expected_error=None, destination=None):
        request_path = requests / (name+'.json')
        request_path.write_text(json.dumps(request)+'\n')
        frozen[str(request_path)] = sha(request_path)
        destination = destination or output / (name+'.json')
        command = [str(binary),'nif-animation',str(source_path),'--markers-request',str(request_path),'--output',str(destination)]
        process = subprocess.run(command,capture_output=True,text=True)
        if expected_error in ('schema','protected'):
            assert process.returncode != 0 and not destination.exists(), process.stdout+process.stderr
        else:
            report = json.loads(destination.read_text())
            assert process.returncode == (0 if expected_error is None else 1), process.stderr
            if expected_error:
                assert report['evaluation'] is None and report['failures']==1 and expected_error in report['error'],report
            else:
                value=report['evaluation']
                assert value['source_sha256']==sha(source_path)
                assert value['source_start_f64_bits']==struct.unpack('<Q',struct.pack('<d',request['source_start']))[0]
                assert value['source_end_f64_bits']==struct.unpack('<Q',struct.pack('<d',request['source_end']))[0]
                assert not value['retail_behavior_verified']
        invocations.append(dict(command=command,exit_code=process.returncode,expected_error=expected_error,
                                receipt_sha256=sha(destination) if destination.exists() else None))
        return None if expected_error else value
    full=run('full',base.copy())
    assert [(entry['source_key_ordinal'],entry['time_bits'],bytes(entry['raw_string_bytes'])) for entry in full['entries']]==[
        (0,0x40000000,b'a\0b\0\0'),(1,0x80000000,b'\r\n"\\'),(2,0x40000000,b'\x80\xff')]
    point=run('point',dict(base,source_start=2,source_end=2))
    assert [entry['source_key_ordinal'] for entry in point['entries']]==[0,2]
    empty=run('empty',dict(base,source_start=3,source_end=4));assert empty['entries']==[]
    signed=run('signed-zero',dict(base,source_start=0.,source_end=-0.));assert [entry['source_key_ordinal'] for entry in signed['entries']]==[1]
    stale=base['expected_sha256'].copy();stale[0]^=1
    run('stale',dict(base,expected_sha256=stale),expected_error='SHA256 differs')
    run('wrong-sequence',dict(base,sequence=3),expected_error='sequence 3 is not decoded')
    run('reversed',dict(base,source_start=3,source_end=2),expected_error='finite ordered bounds')
    for name,offset,word,error in [
        ('missing-link',74,0xffffffff,'sequence 1 has no text_keys link'),
        ('nonfinite',24,0x7fc00000,'nonfinite NIF float'),
        ('null-string',20,0xffffffff,'text_keys 3 key 1 has no authored string')]:
        changed=copy.deepcopy(blocks)
        block=1 if name=='missing-link' else 3
        payload=bytearray(changed[block][1]);payload[offset:offset+4]=fixtures.w(word)
        changed[block]=(changed[block][0],bytes(payload))
        path=inputs/(name+'.kf');path.write_bytes(fixtures.container(34,changed,strings));frozen[str(path)]=sha(path)
        request=dict(base,expected_sha256=list(bytes.fromhex(sha(path))))
        run(name,request,source_path=path,expected_error=error)
    run('extra-field',dict(base,extra=1),expected_error='schema')
    run('schema',dict(base,schema_version=2),expected_error='schema')
    run('protected-request',base.copy(),expected_error='protected',destination=requests/'blocked-output.json')
    run('protected-source',base.copy(),expected_error='protected',destination=inputs/'blocked-output.json')
    original_checks=[]
    if original is not None:
        assert native is not None
        frozen.update({str(path):sha(path) for path in [original,native]})
        oracle=json.loads(native.read_text())
        row=next(row for row in oracle['files'] if row['sha256']==sha(original))
        assert oracle['nifly_revision']=='cca0a770094bb962fb28ea1fec5ea903e68fda8e'
        assert oracle['prepare_data_called'] is False
        assert oracle['raw_string_table_checked'] is True and oracle['raw_count_fields_checked'] is True
        assert row['trailing_nul_normalizations']==0
        selected=next(block['data']['sequence'] for block in row['animations'] if block['block']==0)
        text_id=selected['text_keys']
        text=next(block['data']['text_keys'] for block in row['animations'] if block['block']==text_id)
        def as_float(bits):return struct.unpack('<f',struct.pack('<I',bits))[0]
        for label,start,end in [('original-full',-1,4),('original-point',0,0),
                                ('original-last',as_float(text['keys'][-1]['time_bits']),as_float(text['keys'][-1]['time_bits']))]:
            request=dict(base,expected_sha256=list(bytes.fromhex(sha(original))),sequence=0,source_start=start,source_end=end)
            value=run(label,request,source_path=original)
            expected=[(ordinal,key['time_bits'],key['value'],row['strings'][key['value']]) for ordinal,key in enumerate(text['keys']) if start<=as_float(key['time_bits'])<=end]
            actual=[(entry['source_key_ordinal'],entry['time_bits'],entry['string_index'],entry['raw_string_bytes']) for entry in value['entries']]
            assert actual==expected,(actual,expected)
            assert value['text_keys']['block']==text_id
            for field,block_id in [('sequence',0),('text_keys',text_id)]:
                source_block=next(block for block in row['animations'] if block['block']==block_id)
                assert value[field]=={key:source_block[key] for key in ('block','offset','bytes','sha256')}
            original_checks.append(dict(label=label,sequence=0,text_keys=text_id,physical_entries=len(expected)))
    for path,digest in frozen.items():assert sha(Path(path))==digest
    result=dict(contract='engineering-source-text-key-interval-v1',authored_queries=4,intended_refusals=10,
                original_queries=original_checks,invocations=invocations,frozen_hashes=frozen,retail_behavior_verified=False)
    (output/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--original',type=Path)
    parser.add_argument('--native',type=Path)
    arguments=parser.parse_args()
    result=check(arguments.binary,arguments.output,arguments.original,arguments.native)
    print(json.dumps({key:value for key,value in result.items() if key not in ('invocations','frozen_hashes')}))
