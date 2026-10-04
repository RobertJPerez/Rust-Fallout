// SPDX-License-Identifier: GPL-3.0-only
// Isolated pinned raw factories; no NifFile::Load, PrepareData or evaluation.
#include "NifFile.hpp"
#include "Animation.hpp"
#include "ExtraData.hpp"
#include <windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <cmath>
#include <cctype>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string_view>
#include <vector>
using namespace nifly;
// Keep per-file failure rows valid without admitting unbounded JSON buffering.
class RowBuffer : public std::streambuf {
    std::string value;
    static constexpr size_t maximum = 64 * 1024 * 1024;
    void append(const char* data, size_t size) {
        if(size > maximum-value.size())throw std::runtime_error("native animation row exceeds JSON byte budget");
        value.append(data,size);
    }
protected:
    std::streamsize xsputn(const char* data,std::streamsize size) override {append(data,static_cast<size_t>(size));return size;}
    int_type overflow(int_type c) override {if(!traits_type::eq_int_type(c,traits_type::eof())){const char byte=traits_type::to_char_type(c);append(&byte,1);}return traits_type::not_eof(c);}
public:
    const std::string& get() const {return value;}
};
static std::string snapshot(const std::filesystem::path& path) {
    std::ifstream input(path,std::ios::binary|std::ios::ate);const auto length=input.tellg();
    if(!input||length<0||length>64*1024*1024) throw std::runtime_error("native animation input exceeds64MiB");
    std::string data(static_cast<size_t>(length),'\0');input.seekg(0);input.read(data.data(),length);
    if(!input) throw std::runtime_error("native animation snapshot failed");return data;
}
static std::string sha256(const char* data,size_t length) {
    BCRYPT_ALG_HANDLE algorithm=nullptr;
    if(BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,nullptr,0)<0) throw std::runtime_error("SHA256 provider failed");
    unsigned char digest[32]{};const auto status=BCryptHash(algorithm,nullptr,0,reinterpret_cast<PUCHAR>(const_cast<char*>(data)),static_cast<ULONG>(length),digest,32);
    BCryptCloseAlgorithmProvider(algorithm,0);if(status<0) throw std::runtime_error("SHA256 failed");
    std::ostringstream out;out<<std::hex<<std::setfill('0');for(auto byte:digest)out<<std::setw(2)<<unsigned(byte);return out.str();
}
static void text(std::ostream& out,std::string_view value) {
    out<<'"';for(unsigned char c:value) {
        if(c=='"'||c=='\\')out<<'\\'<<char(c);
        else if(c<0x20) { const char* digits="0123456789abcdef";out<<"\\u00"<<digits[c>>4]<<digits[c&15]; }
        else out<<char(c);
    }out<<'"';
}
static uint32_t bits(float value) {uint32_t result;std::memcpy(&result,&value,4);return result;}
static void ref(std::ostream& out,uint32_t value) {if(value==UINT32_MAX)out<<"null";else out<<value;}
static void field(std::ostream& out,const char* name,uint32_t value) {out<<',';text(out,name);out<<':';ref(out,value);}
static void vec(std::ostream& out,const Vector3& v) {out<<'['<<bits(v.x)<<','<<bits(v.y)<<','<<bits(v.z)<<']';}
static bool selected(const std::string& name) {return name=="NiTransformController"||name=="NiControllerSequence"||name=="NiTransformInterpolator"||name=="NiTextKeyExtraData";}
#include "raw.hpp"
#include "keyframes.hpp"
#include "splines.hpp"
#include "components.hpp"
#include "booleans.hpp"
#include "boolean_keys.hpp"
static void write_file(std::ostream& out,const std::filesystem::path& path,bool include_keys,bool include_splines,bool include_components,bool include_booleans,bool include_bool_keys) {
    const auto bytes=snapshot(path);const auto raw=raw_header(bytes);
    std::istringstream input(bytes,std::ios::binary);NiHeader header;NiIStream stream(&input,&header);header.Get(stream);
    if(!input||!header.IsValid()||header.GetNumBlocks()!=raw.sizes.size()||input.tellg()!=static_cast<std::streamoff>(raw.payload_start)||header.GetStringCount()!=raw.strings.size())
        throw std::runtime_error("native/raw header interpretation differs");
    size_t normalizations=0;
    for(uint32_t i=0;i<raw.strings.size();++i) {
        const auto& original=raw.strings[i];const bool trailing=!original.empty()&&original.back()=='\0';
        const std::string_view expected(original.data(),original.size()-size_t(trailing));
        if(header.GetStringById(i)!=expected) throw std::runtime_error("native raw-string interpretation differs");normalizations+=trailing;
    }
    std::vector<std::unique_ptr<NiObject>> blocks;blocks.reserve(raw.sizes.size());
    std::vector<RawCounts> counts;counts.reserve(raw.sizes.size());size_t remaining=128*1024*1024;
    storage(remaining,raw.sizes.size(),sizeof(RawCounts));
    std::vector<KeyCounts> key_counts;
    size_t key_checks=16000000;
    if(include_keys){storage(remaining,raw.sizes.size(),sizeof(KeyCounts));key_counts.resize(raw.sizes.size());}
    std::vector<SplineCounts> spline_counts;size_t spline_checks=16000000,component_checks=16000000,boolean_checks=16000000,boolean_key_checks=16000000;
    std::vector<uint32_t> boolean_key_counts;
    if(include_bool_keys){storage(remaining,raw.sizes.size(),sizeof(uint32_t));boolean_key_counts.resize(raw.sizes.size());}
    if(include_splines){storage(remaining,raw.sizes.size(),sizeof(SplineCounts));spline_counts.resize(raw.sizes.size());}
    for(uint32_t id=0;id<raw.sizes.size();++id) {
        const auto& name=raw.types[raw.indices[id]];
        if(header.GetBlockTypeStringById(id)!=name||header.GetBlockSize(id)!=raw.sizes[id])throw std::runtime_error("native/raw block table differs");
        const auto payload=std::string_view(bytes).substr(raw.offsets[id],raw.sizes[id]);
        counts.push_back(selected(name)?preflight(payload,name,raw,remaining):RawCounts{});
        const bool key_data=include_keys&&name=="NiTransformData";
        if(key_data)key_counts[id]=preflight_keys(payload,remaining,key_checks);
        const bool spline_data=include_splines&&selected_spline(name);
        if(spline_data)spline_counts[id]=preflight_spline(payload,name,raw,remaining,spline_checks);
        const bool component_data=include_components&&selected_component(name);
        if(component_data)preflight_component(payload,name,raw,remaining,component_checks);
        const bool boolean_data=include_booleans&&selected_boolean(name);
        if(boolean_data)preflight_boolean(payload,name,raw,remaining,boolean_checks);
        const bool bool_keys=include_bool_keys&&name=="NiBoolData";
        if(bool_keys)boolean_key_counts[id]=preflight_boolean_keys(payload,remaining,boolean_key_checks);
        if(selected(name)||key_data||spline_data||component_data||boolean_data||bool_keys) {
            std::istringstream block_input(std::string(payload),std::ios::binary);NiIStream block_stream(&block_input,&header);
            const auto factory=NiFactoryRegister::Get().GetFactoryByName(name);if(!factory)throw std::runtime_error("required animation factory missing");
            blocks.push_back(factory->Load(block_stream));
            if(!block_input||block_input.tellg()!=static_cast<std::streamoff>(payload.size()))throw std::runtime_error("native animation factory span differs");
        }else blocks.push_back(nullptr);
    }
    input.seekg(static_cast<std::streamoff>(raw.footer_offset));header.GetFooter(stream);
    if(!input||input.peek()!=std::char_traits<char>::eof())throw std::runtime_error("native animation footer differs");header.SetBlockReference(&blocks);
    out<<"{\"file\":";text(out,path.filename().u8string());out<<",\"decoded_bytes\":"<<bytes.size()<<",\"sha256\":";text(out,sha256(bytes.data(),bytes.size()));
    out<<",\"version\":335675399,\"user_version\":11,\"bethesda_version\":"<<raw.stream<<",\"strings\":[";
    for(size_t i=0;i<raw.strings.size();++i) {if(i)out<<',';out<<'[';for(size_t j=0;j<raw.strings[i].size();++j){if(j)out<<',';out<<unsigned(static_cast<unsigned char>(raw.strings[i][j]));}out<<']';}
    out<<"],\"trailing_nul_normalizations\":"<<normalizations<<",\"animations\":[";bool first=true;
    for(uint32_t id=0;id<blocks.size();++id) {
        const auto& name=raw.types[raw.indices[id]];if(!selected(name))continue;if(!first)out<<',';first=false;
        out<<"{\"block\":"<<id<<",\"block_type\":";text(out,name);out<<",\"offset\":"<<raw.offsets[id]<<",\"bytes\":"<<raw.sizes[id]<<",\"sha256\":";text(out,sha256(bytes.data()+raw.offsets[id],raw.sizes[id]));out<<",\"data\":";
        if(auto c=header.GetBlock<NiTransformController>(id)) {
            out<<"{\"kind\":\"transform_controller\",\"controller\":{\"next_controller\":";ref(out,c->nextControllerRef.index);
            out<<",\"flags\":"<<c->flags<<",\"frequency_bits\":"<<bits(c->frequency)<<",\"phase_bits\":"<<bits(c->phase)<<",\"start_bits\":"<<bits(c->startTime)<<",\"stop_bits\":"<<bits(c->stopTime);
            field(out,"target",c->targetRef.index);field(out,"interpolator",c->interpolatorRef.index);out<<"}}";
        } else if(auto s=header.GetBlock<NiControllerSequence>(id)) {
            if(s->controlledBlocks.size()!=counts[id].records)throw std::runtime_error("native controlled-block count differs");
            out<<"{\"kind\":\"controller_sequence\",\"sequence\":{\"name\":";ref(out,s->name.GetIndex());
            out<<",\"declared_controlled_blocks\":"<<counts[id].records<<",\"array_grow_by\":"<<s->arrayGrowBy<<",\"controlled_blocks\":[";
            for(size_t i=0;i<s->controlledBlocks.size();++i) {
                if(i)out<<',';const auto& p=s->controlledBlocks[i];out<<"{\"interpolator\":";ref(out,p.interpolatorRef.index);field(out,"controller",p.controllerRef.index);out<<",\"priority\":"<<unsigned(p.priority);
                field(out,"node_name",p.nodeName.GetIndex());field(out,"property_type",p.propType.GetIndex());field(out,"controller_type",p.ctrlType.GetIndex());field(out,"controller_id",p.ctrlID.GetIndex());field(out,"interpolator_id",p.interpID.GetIndex());out<<'}';
            }
            out<<"],\"weight_bits\":"<<bits(s->weight);field(out,"text_keys",s->textKeyRef.index);out<<",\"cycle_type\":"<<static_cast<uint32_t>(s->cycleType)<<",\"frequency_bits\":"<<bits(s->frequency)<<",\"start_bits\":"<<bits(s->startTime)<<",\"stop_bits\":"<<bits(s->stopTime);
            field(out,"manager",s->managerRef.index);field(out,"accum_root_name",s->accumRootName.GetIndex());out<<",\"notes\":{\"layout\":";
            if(raw.stream>=24&&raw.stream<=28) {text(out,"single");field(out,"target",s->animNotesRef.index);}
            else if(raw.stream>28) {
                if(s->animNotesRefs.GetSize()!=counts[id].notes)throw std::runtime_error("native notes count differs");
                text(out,"array");out<<",\"declared_count\":"<<counts[id].notes<<",\"targets\":[";
                for(uint32_t i=0;i<counts[id].notes;++i){if(i)out<<',';ref(out,s->animNotesRefs.GetBlockRef(i));}out<<']';
            }else text(out,"absent");out<<"}}}";
        } else if(auto t=header.GetBlock<NiTransformInterpolator>(id)) {
            out<<"{\"kind\":\"transform_interpolator\",\"interpolator\":{\"translation_bits\":";vec(out,t->translation);
            out<<",\"rotation_wxyz_bits\":["<<bits(t->rotation.w)<<','<<bits(t->rotation.x)<<','<<bits(t->rotation.y)<<','<<bits(t->rotation.z)<<"],\"scale_bits\":"<<bits(t->scale);field(out,"data",t->dataRef.index);out<<"}}";
        } else if(auto t=header.GetBlock<NiTextKeyExtraData>(id)) {
            if(t->textKeys.size()!=counts[id].records)throw std::runtime_error("native text-key count differs");
            out<<"{\"kind\":\"text_key_extra_data\",\"text_keys\":{\"name\":";ref(out,t->name.GetIndex());out<<",\"declared_keys\":"<<counts[id].records<<",\"keys\":[";
            for(size_t i=0;i<t->textKeys.size();++i){if(i)out<<',';const auto& k=t->textKeys[i];out<<"{\"time_bits\":"<<bits(k.time)<<",\"value\":";ref(out,k.value.GetIndex());out<<'}';}out<<"]}}";
        } else throw std::runtime_error("native selected animation factory type differs");out<<'}';
    }
    out<<']';
    if(include_keys){
        out<<",\"keys\":[";bool first_key=true;
        for(uint32_t id=0;id<blocks.size();++id){
            const auto& name=raw.types[raw.indices[id]];if(name!="NiTransformData")continue;
            if(!first_key)out<<',';first_key=false;
            out<<"{\"block\":"<<id<<",\"block_type\":\"NiTransformData\",\"offset\":"<<raw.offsets[id]<<",\"bytes\":"<<raw.sizes[id]<<",\"sha256\":";
            text(out,sha256(bytes.data()+raw.offsets[id],raw.sizes[id]));out<<",\"data\":";
            const auto data=header.GetBlock<NiTransformData>(id);if(!data)throw std::runtime_error("native transform-key factory type differs");
            project_keys(out,*data,key_counts[id]);out<<'}';
        }out<<']';
    }
    if(include_splines){
        out<<",\"splines\":[";bool first_spline=true;
        for(uint32_t id=0;id<blocks.size();++id){
            const auto& name=raw.types[raw.indices[id]];if(!selected_spline(name))continue;
            if(!first_spline)out<<',';first_spline=false;
            out<<"{\"block\":"<<id<<",\"block_type\":";text(out,name);
            out<<",\"offset\":"<<raw.offsets[id]<<",\"bytes\":"<<raw.sizes[id]<<",\"sha256\":";
            text(out,sha256(bytes.data()+raw.offsets[id],raw.sizes[id]));out<<",\"data\":";
            if(!blocks[id])throw std::runtime_error("native selected spline block missing");
            project_spline(out,*blocks[id],name,spline_counts[id]);out<<'}';
        }out<<']';
    }
    if(include_components){
        out<<",\"spline_components\":[";bool first_component=true;
        for(uint32_t id=0;id<blocks.size();++id){
            const auto& name=raw.types[raw.indices[id]];if(!selected_component(name))continue;
            if(!first_component)out<<',';first_component=false;
            out<<"{\"block\":"<<id<<",\"block_type\":";text(out,name);
            out<<",\"offset\":"<<raw.offsets[id]<<",\"bytes\":"<<raw.sizes[id]<<",\"sha256\":";
            text(out,sha256(bytes.data()+raw.offsets[id],raw.sizes[id]));out<<",\"data\":";
            if(!blocks[id])throw std::runtime_error("native selected component block missing");
            project_component(out,*blocks[id],name);out<<'}';
        }out<<']';
    }
    if(include_booleans){
        out<<",\"bool_interpolators\":[";bool first_boolean=true;
        for(uint32_t id=0;id<blocks.size();++id){
            const auto& name=raw.types[raw.indices[id]];if(!selected_boolean(name))continue;
            if(!first_boolean)out<<',';first_boolean=false;
            out<<"{\"block\":"<<id<<",\"block_type\":";text(out,name);
            out<<",\"offset\":"<<raw.offsets[id]<<",\"bytes\":"<<raw.sizes[id]<<",\"sha256\":";
            text(out,sha256(bytes.data()+raw.offsets[id],raw.sizes[id]));out<<",\"data\":";
            if(!blocks[id])throw std::runtime_error("native selected Boolean block missing");
            project_boolean(out,*blocks[id]);out<<'}';
        }out<<']';
    }
    if(include_bool_keys){
        out<<",\"bool_keys\":[";bool first_key=true;
        for(uint32_t id=0;id<blocks.size();++id){
            const auto& name=raw.types[raw.indices[id]];if(name!="NiBoolData")continue;
            if(!first_key)out<<',';first_key=false;
            out<<"{\"block\":"<<id<<",\"block_type\":\"NiBoolData\",\"offset\":"<<raw.offsets[id];
            out<<",\"bytes\":"<<raw.sizes[id]<<",\"sha256\":";text(out,sha256(bytes.data()+raw.offsets[id],raw.sizes[id]));out<<",\"data\":";
            if(!blocks[id])throw std::runtime_error("native selected Boolean-key block missing");
            project_boolean_keys(out,*blocks[id],boolean_key_counts[id]);out<<'}';
        }out<<']';
    }out<<'}';
}
int main(int argc,char** argv) {
    const bool include_bool_keys=argc==3&&std::string_view(argv[2])=="--include-bool-keys";
    const bool include_booleans=include_bool_keys||(argc==3&&std::string_view(argv[2])=="--include-bool-interpolators");
    const bool include_components=include_booleans||(argc==3&&std::string_view(argv[2])=="--include-spline-components");
    const bool include_splines=include_components||(argc==3&&std::string_view(argv[2])=="--include-splines");
    const bool include_keys=include_splines||(argc==3&&std::string_view(argv[2])=="--include-keyframes");
    if(argc!=2&&!include_keys){std::cerr<<"usage: nif-animation-oracle INPUT_FILE_OR_DIRECTORY [--include-keyframes|--include-splines|--include-spline-components|--include-bool-interpolators|--include-bool-keys]\n";return 2;}
    try {
        const std::filesystem::path input(argv[1]);std::vector<std::filesystem::path> paths;
        if(std::filesystem::is_directory(input)) {
            for(const auto& entry:std::filesystem::directory_iterator(input))if(entry.is_regular_file()){
                auto ext=entry.path().extension().string();std::transform(ext.begin(),ext.end(),ext.begin(),[](unsigned char c){return char(std::tolower(c));});
                if(ext==".blob"||ext==".nif"||ext==".kf"){if(paths.size()==10000)throw std::runtime_error("native animation file count exceeds budget");paths.push_back(entry.path());}
            }
        }else paths.push_back(input);if(paths.empty())throw std::runtime_error("native animation found no inputs");std::sort(paths.begin(),paths.end());
        const auto binary=snapshot(argv[0]);
        std::cout<<"{\"schema_version\":"<<(include_bool_keys?6:include_booleans?5:include_components?4:include_splines?3:include_keys?2:1)<<",\"animation_branch\":\"nv-four-source-classes\",\"float_encoding\":\"ieee754-binary32-bits\",\"string_encoding\":\"raw-byte-arrays\",\"nifly_revision\":\"cca0a770094bb962fb28ea1fec5ea903e68fda8e\",\"prepare_data_called\":false,\"raw_string_table_checked\":true,\"raw_count_fields_checked\":true,\"runtime_ready\":false";
        if(include_keys)std::cout<<",\"keyframe_branch\":\"nv-transform-data-source\",\"raw_keyframe_counts_checked\":true";
        if(include_splines)std::cout<<",\"spline_branch\":\"nv-compact-transform-source\",\"raw_spline_counts_checked\":true";
        if(include_components)std::cout<<",\"spline_component_branch\":\"nv-compact-components-source\",\"raw_component_fields_checked\":true";
        if(include_booleans)std::cout<<",\"bool_interpolator_branch\":\"nv-bool-interpolator-source\",\"raw_bool_fields_checked\":true";
        if(include_bool_keys)std::cout<<",\"bool_key_branch\":\"nv-bool-constant-key-source\",\"raw_bool_key_counts_checked\":true";
        std::cout<<",\"oracle_binary_sha256\":";text(std::cout,sha256(binary.data(),binary.size()));std::cout<<",\"files\":[";
        bool failed=false;
        for(size_t i=0;i<paths.size();++i){if(i)std::cout<<',';try{RowBuffer buffer;std::ostream row(&buffer);row.exceptions(std::ios::badbit|std::ios::failbit);write_file(row,paths[i],include_keys,include_splines,include_components,include_booleans,include_bool_keys);std::cout<<buffer.get();}catch(const std::exception& e){failed=true;std::cout<<"{\"file\":";text(std::cout,paths[i].filename().u8string());std::cout<<",\"error\":";text(std::cout,e.what());std::cout<<'}';}}
        std::cout<<"]}\n";return failed?1:0;
    }catch(const std::exception& e){std::cerr<<"animation oracle: "<<e.what()<<'\n';return 1;}
}
