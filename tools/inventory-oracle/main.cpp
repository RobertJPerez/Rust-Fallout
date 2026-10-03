// Original offline inventory projection. Original headers, source namespaces and
// compressed bodies are read independently; no live inventory is initialized.
#include "../oracle-common/record_source.hpp"
#include "../oracle-common/zlib_source.hpp"
using namespace fallout_records;

static std::string kind_bytes(const Bytes& bytes, size_t offset=0) {
    std::vector<std::string> parts;
    for (size_t n=0;n<4;++n) parts.push_back(std::to_string(integer(bytes,offset+n,1)));
    return array(parts);
}
static std::string signed_word(uint32_t word) {
    return std::to_string(word & 0x80000000u ? static_cast<int64_t>(word)-0x100000000LL : static_cast<int64_t>(word));
}
struct Binding {
    std::string json, status;
    std::optional<std::string> kind;
};
struct Counts {
    std::map<std::string,uint64_t> numbers{{"records",0},{"deleted_records",0},{"decoded_bytes",0},{"fields",0},{"items",0},{"extra_fields",0},{"template_fields",0},{"non_positive_counts",0},{"source_findings",0}};
    std::map<std::string,uint64_t> kinds, bindings;
    static std::string map_json(const std::map<std::string,uint64_t>& values) {
        Object result; for (const auto& value:values) result[value.first]=std::to_string(value.second); return object(result);
    }
    std::string json() const {
        Object result; for (const auto& value:numbers) result[value.first]=std::to_string(value.second);
        result["record_kinds"]=map_json(kinds); result["binding_statuses"]=map_json(bindings); return object(result);
    }
};
static Binding bind(const HeaderIndex& index, size_t source_index, uint32_t raw, Counts& counts) {
    const auto& source=index.plugins[source_index];
    const auto key=resolve(source.name,source.masters,raw);
    const auto winner=key ? index.winners.find(*key) : index.winners.end();
    std::string status="null", target="null"; std::optional<std::string> kind;
    if (key) {
        if (winner==index.winners.end()) status="missing";
        else {
            const auto& entry=winner->second;
            const auto flags=static_cast<uint32_t>(integer(entry.header,8,4));
            status=flags & 0x20 ? "deleted" : "defined";
            kind=std::string(entry.header.begin(),entry.header.begin()+4);
            target=object({{"kind",kind_bytes(entry.header)},{"source_plugin",quote(index.plugins[entry.source].name)},
                {"record_file_offset",std::to_string(entry.offset)},{"record_flags",std::to_string(flags)}});
        }
    }
    ++counts.bindings[status];
    return {object({{"raw_form",std::to_string(raw)},{"key",key ? key->json() : "null"},{"status",quote(status)},{"target",target}}),status,kind};
}
struct Item { size_t cnto; std::vector<std::string> coed; };
struct Document { std::vector<std::string> fields, findings; std::vector<Item> items; };
static Document decode(const HeaderIndex& index, const Entry& entry, const Bytes& body, const std::string& record_kind, Counts& counts) {
    Document result; std::optional<size_t> extended, pending; size_t actor_headers=0, templates=0;
    const bool actor=record_kind!="CONT";
    for (size_t cursor=0;cursor<body.size();) {
        if (body.size()-cursor<6 || counts.numbers["fields"]>=2000000) throw std::runtime_error("inventory field header/budget");
        const size_t start=cursor;
        const std::string kind(body.begin()+cursor,body.begin()+cursor+4);
        const auto short_size=static_cast<size_t>(integer(body,cursor+4,2)); cursor+=6;
        if (kind=="XXXX") {
            if (short_size!=4 || extended) throw std::runtime_error("inventory extended size");
            extended=static_cast<size_t>(integer(body,cursor,4)); cursor+=4; continue;
        }
        const auto size=extended.value_or(short_size); extended.reset();
        if (size>body.size()-cursor) throw std::runtime_error("inventory field extent");
        const Bytes data(body.begin()+cursor,body.begin()+cursor+size); cursor+=size;
        const auto exact=[&](size_t required) { if (size!=required) throw std::runtime_error("inventory known field shape"); };
        const auto finding=[&](const std::string& code) {
            result.findings.push_back(object({{"field_decoded_offset",std::to_string(start)},{"code",quote(code)}})); ++counts.numbers["source_findings"];
        };
        std::string value=object({{"kind",quote("opaque")}});
        if (kind=="CNTO") {
            exact(8); if (counts.numbers["items"]>=1000000) throw std::runtime_error("inventory item budget");
            const auto raw=static_cast<uint32_t>(integer(data,0,4)), count=static_cast<uint32_t>(integer(data,4,4));
            auto item=bind(index,entry.source,raw,counts);
            std::string allowed="null";
            if (item.kind) { const std::set<std::string> kinds{"ARMO","AMMO","MISC","WEAP","BOOK","LVLI","KEYM","ALCH","NOTE","IMOD","CMNY","CCRD","LIGH","CHIP"}; allowed=kinds.count(*item.kind) ? "true" : "false"; }
            value=object({{"kind",quote("item")},{"item",item.json},{"count",signed_word(count)},{"schema_kind_allowed",allowed}});
            pending=result.items.size(); result.items.push_back({result.fields.size(),{}});
            ++counts.numbers["items"]; if (!count || (count & 0x80000000u)) ++counts.numbers["non_positive_counts"];
        } else if (kind=="COED") {
            exact(12); ++counts.numbers["extra_fields"];
            if (pending) {
                auto& item=result.items[*pending]; if (!item.coed.empty()) finding("multiple_item_extra_fields");
                item.coed.push_back(std::to_string(result.fields.size()));
            } else finding("orphan_item_extra_field");
            const auto raw=static_cast<uint32_t>(integer(data,0,4)), word=static_cast<uint32_t>(integer(data,4,4));
            const auto owner=bind(index,entry.source,raw,counts);
            std::string extra;
            if (owner.status=="null") extra=object({{"kind",quote("unused")},{"raw_word",std::to_string(word)}});
            else if (owner.status=="defined" && owner.kind=="NPC_") extra=object({{"kind",quote("global")},{"binding",bind(index,entry.source,word,counts).json}});
            else if (owner.status=="defined" && owner.kind=="FACT") extra=object({{"kind",quote("required_rank")},{"raw_word",std::to_string(word)},{"value",signed_word(word)}});
            else extra=object({{"kind",quote("unresolved_owner")},{"raw_word",std::to_string(word)}});
            value=object({{"kind",quote("extra")},{"owner",owner.json},{"union_word",extra},{"condition_bits",std::to_string(integer(data,8,4))}});
        } else if (kind=="ACBS" && actor) {
            exact(24); pending.reset(); if (++actor_headers>1) finding("multiple_actor_base_fields");
            const auto flags=integer(data,0,4), template_flags=integer(data,22,2);
            value=object({{"kind",quote("actor_base")},{"flags",std::to_string(flags)},{"template_flags",std::to_string(template_flags)},{"inventory_template_flag",template_flags & 0x100 ? "true":"false"}});
        } else if (kind=="TPLT" && actor) {
            exact(4); pending.reset(); if (++templates>1) finding("multiple_actor_template_fields"); ++counts.numbers["template_fields"];
            value=object({{"kind",quote("template")},{"template",bind(index,entry.source,static_cast<uint32_t>(integer(data,0,4)),counts).json}});
        } else if (kind=="DATA" && !actor) {
            exact(5); pending.reset(); const auto flags=integer(data,0,1);
            value=object({{"kind",quote("container_data")},{"flags",std::to_string(flags)},{"weight_bits",std::to_string(integer(data,1,4))},{"respawn_flag",flags & 2 ? "true":"false"}});
        } else pending.reset();
        result.fields.push_back(object({{"kind",kind_bytes(body,start)},{"decoded_offset",std::to_string(start)},{"bytes",std::to_string(size)},
            {"sha256",quote(fallout_tables::hash(data))},{"value",value}})); ++counts.numbers["fields"];
    }
    if (extended) throw std::runtime_error("orphan inventory extended prefix");
    return result;
}
int wmain(int argc,wchar_t** argv) {
    try {
        if (argc!=3) throw std::runtime_error("usage: inventory-oracle Data_directory order_bundle");
        auto index=scan(argv[1],argv[2]); Counts counts; std::vector<std::string> definitions;
        for (const auto& winner:index.winners) {
            const auto& entry=winner.second; const auto& source=index.plugins[entry.source];
            const std::string kind(entry.header.begin(),entry.header.begin()+4);
            if (kind!="CONT" && kind!="NPC_" && kind!="CREA") continue;
            if (++counts.numbers["records"]>65536) throw std::runtime_error("inventory record budget"); ++counts.kinds[kind];
            const auto flags=static_cast<uint32_t>(integer(entry.header,8,4)); const bool deleted=flags & 0x20;
            Document document; std::string body_sha="null";
            if (deleted) ++counts.numbers["deleted_records"];
            else {
                auto body=source.source->read(entry.offset+24,static_cast<size_t>(integer(entry.header,4,4)));
                if (flags & 0x40000) {
                    const auto expected=static_cast<size_t>(integer(body,0,4)); const Bytes frame(body.begin()+4,body.end());
                    auto decoded=fallout_zlib::decode(frame,expected);
                    if (decoded.stored_adler!=decoded.calculated_adler) throw std::runtime_error("inventory compressed checksum"); body=std::move(decoded.payload);
                }
                counts.numbers["decoded_bytes"]+=body.size();
                if (body.size()>64*1024*1024 || counts.numbers["decoded_bytes"]>256ULL*1024*1024) throw std::runtime_error("inventory decoded byte budget");
                body_sha=quote(fallout_tables::hash(body)); document=decode(index,entry,body,kind,counts);
            }
            std::vector<std::string> items;
            for (const auto& item:document.items) items.push_back(object({{"cnto_field",std::to_string(item.cnto)},{"coed_fields",array(item.coed)}}));
            definitions.push_back(object({{"key",winner.first.json()},{"kind",kind_bytes(entry.header)},
                {"source",object({{"plugin",quote(source.name)},{"sha256",quote(source.source->sha256)},{"record_file_offset",std::to_string(entry.offset)},
                    {"record_flags",std::to_string(flags)},{"decoded_record_sha256",body_sha}})},
                {"deleted",deleted ? "true":"false"},{"fields",array(document.fields)},{"items",array(items)},{"findings",array(document.findings)}}));
        }
        std::cout<<object({{"schema_version","1"},{"profile",quote("nv-original")},{"sources",index.sources_json()},
            {"metadata",index.metadata_json()},{"counts",counts.json()},{"definitions",array(definitions)}})<<'\n';
        return 0;
    } catch (const std::exception& error) { std::cerr<<"inventory-oracle: "<<error.what()<<'\n'; return 1; }
}
