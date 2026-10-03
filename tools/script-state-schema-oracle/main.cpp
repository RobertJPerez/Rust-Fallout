// Original offline schema reader. It reads and independently decompresses
// winning source bodies; it neither loads original code nor initializes gameplay.
#include "../oracle-common/record_source.hpp"
#include "../oracle-common/zlib_source.hpp"
using namespace fallout_records;

static bool script_kind(const std::string& kind) {
    const std::set<std::string> kinds{"SCPT","INFO","QUST","PACK","PERK","TERM","REFR","ACHR","ACRE","PGRE","PMIS","PBEA"};
    return kinds.count(kind) != 0;
}
static std::string kind_json(uint32_t index, uint8_t type, bool reference) {
    if (index == 0) return object({{"kind",quote("unverified_zero_index")},{"type_byte",std::to_string(type)}});
    if (reference) return object({{"kind",quote("reference")}});
    if (type < 2) return object({{"kind",quote(type ? "integer" : "float")}});
    return object({{"kind",quote("unsupported")},{"type_byte",std::to_string(type)}});
}
struct Schema {
    std::map<uint32_t,fallout_tables::Variable> variables;
    std::set<uint32_t> references;
    uint64_t duplicates=0;
};
static Schema schema(const std::vector<fallout_tables::Field>& fields) {
    Schema result; std::optional<fallout_tables::Field> declaration;
    bool compiled=false, text=false; size_t variable_count=0, reference_count=0;
    for (const auto& field : fields) {
        if (declaration && field.kind!="SCVR") throw std::runtime_error("unnamed compiled declaration");
        if (field.kind=="SLSD") {
            if (field.data.size()!=24 || ++variable_count>65536) throw std::runtime_error("declaration shape/budget");
            declaration=field;
        } else if (field.kind=="SCVR") {
            if (!declaration || field.data.empty() || field.data.back()!=0
                || std::find(field.data.begin(),field.data.end()-1,0)!=field.data.end()-1) throw std::runtime_error("declaration name pairing/termination");
            const auto index=static_cast<uint32_t>(integer(declaration->data,0,4));
            if (!result.variables.emplace(index,fallout_tables::Variable{declaration->data,field.data,declaration->offset}).second) ++result.duplicates;
            declaration.reset();
        } else if (field.kind=="SCRV" || field.kind=="SCRO") {
            if (field.data.size()!=4 || ++reference_count>65536) throw std::runtime_error("reference shape/budget");
            if (field.kind=="SCRV") { const auto target=static_cast<uint32_t>(integer(field.data,0,4)); if (target) result.references.insert(target); }
        } else if (field.kind=="SCTX") {
            if (text) throw std::runtime_error("duplicate source text"); text=true;
        } else if (field.kind=="SCDA") {
            if (compiled || field.data.size()>4*1024*1024) throw std::runtime_error("compiled shape/budget"); compiled=true;
            size_t instruction_count=0;
            for (size_t cursor=0;cursor<field.data.size();) {
                if (++instruction_count>262144) throw std::runtime_error("compiled instruction budget");
                const bool call=integer(field.data,cursor,2)==0x1c; const size_t header=call?8:4;
                const auto opcode=integer(field.data,cursor+(call?4:0),2);
                const auto length=static_cast<size_t>(integer(field.data,cursor+header-2,2)); cursor+=header;
                if (length>field.data.size()-cursor || (opcode==0x10 && length<6)) throw std::runtime_error("compiled instruction extent");
                cursor+=length;
            }
        }
    }
    if (declaration) throw std::runtime_error("unnamed final declaration");
    return result;
}
int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 3) throw std::runtime_error("usage: script-state-schema-oracle Data_directory FRORDER1_bundle");
        auto index = scan(argv[1], argv[2]);
        uint64_t candidates=0, deleted=0, bytes=0, scripts=0, locals=0, duplicates=0;
        std::map<std::string,uint64_t> kinds; std::vector<std::string> records;
        for (const auto& item : index.winners) {
            const auto& entry=item.second; const std::string kind(entry.header.begin(),entry.header.begin()+4);
            if (!script_kind(kind)) continue;
            if (++candidates > 1000000) throw std::runtime_error("script candidate budget");
            const auto flags=integer(entry.header,8,4); if (flags&0x20) { ++deleted; continue; }
            const auto& source=index.plugins[entry.source];
            auto payload=source.source->read(entry.offset+24,static_cast<size_t>(integer(entry.header,4,4)));
            if (flags&0x40000) {
                const auto expected=static_cast<size_t>(integer(payload,0,4));
                const Bytes encoded(payload.begin()+4,payload.end()); auto decoded=fallout_zlib::decode(encoded,expected);
                if (decoded.stored_adler!=decoded.calculated_adler) throw std::runtime_error("script source checksum mismatch");
                payload=std::move(decoded.payload);
            }
            bytes+=payload.size(); if (bytes>256ULL*1024*1024) throw std::runtime_error("script payload budget");
            const auto units=fallout_tables::units(payload); if (units.empty()) continue;
            std::vector<std::string> rows;
            for (const auto& fields : units) {
                const auto summary=schema(fields);
                const auto& references=summary.references;
                std::vector<std::string> declarations;
                for (const auto& variable : summary.variables) {
                    const auto type=variable.second.declaration[16];
                    const auto classified=kind_json(variable.first,type,references.count(variable.first)!=0);
                    const std::string name=variable.first==0 ? "unverified_zero_index" : references.count(variable.first) ? "reference" : type<2 ? type ? "integer" : "float" : "unsupported";
                    ++kinds[name]; ++locals;
                    declarations.push_back(object({{"index",std::to_string(variable.first)},
                        {"declaration_decoded_offset",std::to_string(variable.second.offset)},{"kind",classified}}));
                }
                if (++scripts>262144 || locals>262144) throw std::runtime_error("script schema budget");
                duplicates+=summary.duplicates;
                rows.push_back(object({{"header_decoded_offset",std::to_string(fields.front().offset)},{"locals",array(declarations)}}));
            }
            records.push_back(object({{"key",item.first.json()},{"source_name",quote(source.name)},{"record_kind",quote(kind)},
                {"record_file_offset",std::to_string(entry.offset)},{"record_flags",std::to_string(flags)},
                {"decoded_bytes",std::to_string(payload.size())},{"decoded_sha256",quote(fallout_tables::hash(payload))},{"units",array(rows)}}));
        }
        Object kind_counts; for (const auto& item : kinds) kind_counts[item.first]=std::to_string(item.second);
        const auto counts=object({{"candidate_records",std::to_string(candidates)},{"deleted_candidate_records",std::to_string(deleted)},
            {"decoded_candidate_bytes",std::to_string(bytes)},{"scripts",std::to_string(scripts)},{"unique_locals",std::to_string(locals)},
            {"duplicate_declarations",std::to_string(duplicates)},{"local_kinds",object(kind_counts)}});
        std::cout<<object({{"schema_version","1"},{"profile",quote("nv-original")},{"sources",index.sources_json()},
            {"metadata",index.metadata_json()},{"counts",counts},{"records",array(records)},
            {"constructor_defaults_verified","false"},{"retail_parity_accepted","false"}})<<'\n';
        return 0;
    } catch(const std::exception& error) { std::cerr<<"script-state-schema-oracle: "<<error.what()<<'\n'; return 1; }
}
