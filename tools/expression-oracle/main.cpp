// Original offline expression comparison. SCDA comes from Rust extraction; the
// operator table comes directly from the exact original executable. Tokens and
// statement envelopes are compared, without evaluating a value or native call.
#include "../oracle-common/expression_source.hpp"

struct Body {
    std::vector<std::string> statements;
    size_t expression_bytes=0,tokens=0,trailing_bytes=0,pending=0;
    std::map<uint16_t,uint64_t> commands;
    std::map<uint32_t,uint64_t> operator_codes;
    std::map<uint8_t,uint64_t> kinds;
};
static Body body(const Bytes& data,const std::vector<Operator>& ops) {
    if(data.size()>4*1024*1024)throw std::runtime_error("compiled byte budget");
    Body result;size_t instructions=0;
    for(size_t at=0;at<data.size();) {
        if(++instructions>262144)throw std::runtime_error("instruction budget");
        const size_t start=at;const bool ref=number(data,at,2)==0x1c;
        const size_t header=ref?8:4;
        const auto opcode=number(data,at+(ref?4:0),2),length=number(data,at+header-2,2);
        at+=header;const auto operands=take(data,at,length);
        if(opcode==0x10&&length<6)throw std::runtime_error("short event header");
        if(opcode!=0x15&&opcode!=0x16&&opcode!=0x18)continue;
        Object row{{"instruction_scda_offset",std::to_string(start)},{"opcode",std::to_string(opcode)},
            {"operand_bytes",std::to_string(length)},{"target_kind","null"},{"target_type_byte","null"},
            {"target_index","null"},{"target_context_reference","null"},{"false_jump_bytes","null"},{"issue","null"}};
        size_t cursor=0;
        if(opcode==0x16||opcode==0x18)row["false_jump_bytes"]=std::to_string(word(operands,cursor));
        else {
            auto target=static_cast<uint8_t>(number(operands,cursor++,1));std::optional<uint16_t> context;
            if(target=='r'){context=word(operands,cursor);target=static_cast<uint8_t>(number(operands,cursor++,1));}
            if(target=='f'||target=='s') {
                row["target_kind"]=quote_json("local");row["target_type_byte"]=std::to_string(target);
                row["target_context_reference"]=context?std::to_string(*context):"null";
            }else if(target=='G'&&!context)row["target_kind"]=quote_json("global");
            else throw std::runtime_error("unsupported assignment target");
            row["target_index"]=std::to_string(word(operands,cursor));
        }
        const auto expression_size=word(operands,cursor);
        row["expression_operand_offset"]=std::to_string(cursor);
        const auto expression_bytes=take(operands,cursor,expression_size);
        const auto decoded=expression(expression_bytes,ops);
        const auto tail=take(operands,cursor,operands.size()-cursor);
        row["expression_bytes"]=std::to_string(expression_size);row["tokens"]=std::to_string(decoded.tokens);
        row["token_sha256"]=quote_json(digest(decoded.tuples));
        row["pending_context_reference"]=decoded.pending?std::to_string(*decoded.pending):"null";
        row["trailing_operand_bytes"]=std::to_string(tail.size());row["trailing_operand_sha256"]=quote_json(digest(tail));
        result.statements.push_back(object(row));result.expression_bytes+=expression_size;result.tokens+=decoded.tokens;
        result.trailing_bytes+=tail.size();result.pending+=decoded.pending.has_value();
        merge(result.commands,decoded.commands);merge(result.operator_codes,decoded.operator_codes);merge(result.kinds,decoded.kinds);
    }
    return result;
}
static void compare(const std::filesystem::path& path,const SourceImage& source) {
    std::ifstream input(path,std::ios::binary|std::ios::ate);const auto size=input.tellg();
    if(!input||size<8||size>66*1024*1024)throw std::runtime_error("bundle byte budget");
    Bytes bundle(static_cast<size_t>(size));input.seekg(0);input.read(reinterpret_cast<char*>(bundle.data()),size);
    if(!input||std::string(bundle.begin(),bundle.begin()+8)!="FROBS001")throw std::runtime_error("bundle read or magic");
    std::vector<std::string> operator_rows,bodies;const auto ops=operators(source,operator_rows);
    for(size_t at=8;at<bundle.size();) {
        if(bodies.size()>=262144)throw std::runtime_error("body count budget");
        const auto length=number(bundle,at,4);at+=4;
        if(length>4*1024*1024)throw std::runtime_error("body byte budget");
        const auto data=take(bundle,at,length);const auto decoded=body(data,ops);
        bodies.push_back(object({{"bytes",std::to_string(data.size())},{"sha256",quote_json(digest(data))},
            {"statements",array(decoded.statements)},
            {"counts",object({{"statements",std::to_string(decoded.statements.size())},{"expression_bytes",std::to_string(decoded.expression_bytes)},
                {"tokens",std::to_string(decoded.tokens)},{"trailing_operand_bytes",std::to_string(decoded.trailing_bytes)},
                {"expressions_with_pending_context",std::to_string(decoded.pending)},
                {"command_calls",counts(decoded.commands)},{"operator_codes",counts(decoded.operator_codes)},{"token_kinds",counts(decoded.kinds)}})}}));
    }
    std::cout<<object({{"schema_version","1"},{"bundle_sha256",quote_json(digest(bundle))},
        {"executable_source_sha256",quote_json(source.sha256)},{"operator_descriptors",array(operator_rows)},
        {"bodies",array(bodies)},{"compiled_bodies",std::to_string(bodies.size())},{"execution_ready","false"}})<<'\n';
}
int wmain(int argc,wchar_t** argv) {
    try {
        if(argc!=3)throw std::runtime_error("usage: expression-oracle comparison-bundle FalloutNV.exe");
        const SourceImage source(argv[2]);compare(argv[1],source);return 0;
    }catch(const std::exception& error){std::cerr<<"expression-oracle: "<<error.what()<<'\n';return 1;}
}
