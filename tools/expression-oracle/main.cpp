// Original offline expression comparison. SCDA comes from Rust extraction; the
// operator table comes directly from the exact original executable. Tokens and
// statement envelopes are compared, without evaluating a value or native call.
#include "../oracle-common/pe_source.hpp"
#include <cstdlib>
#include <iostream>
#include <optional>
#include <map>
#include <locale.h>

using fallout_oracle::Bytes;
using fallout_oracle::SourceImage;
using fallout_oracle::digest;
using Object = std::map<std::string,std::string>;
static std::string quote_json(const std::string& value) { std::ostringstream out; out << std::quoted(value); return out.str(); }
static std::string object(const Object& values) {
    std::string out="{"; bool comma=false;
    for (const auto& entry:values) { if(comma)out+=',';comma=true;out+=quote_json(entry.first)+':'+entry.second; }
    return out+'}';
}
static std::string array(const std::vector<std::string>& values) {
    std::string out="["; bool comma=false;
    for (const auto& value:values) { if(comma)out+=',';comma=true;out+=value; } return out+']';
}
static uint32_t number(const Bytes& bytes,size_t at,size_t width) {
    if (at>bytes.size()||width>bytes.size()-at||width>4)throw std::runtime_error("truncated field");
    uint32_t result=0;for(size_t i=0;i<width;++i)result|=uint32_t(bytes[at+i])<<(i*8);return result;
}
static void append(Bytes& bytes,uint32_t value,size_t width) {
    for(size_t i=0;i<width;++i)bytes.push_back(static_cast<unsigned char>(value>>(i*8)));
}
static Bytes take(const Bytes& data,size_t& at,size_t count) {
    if(at>data.size()||count>data.size()-at)throw std::runtime_error("truncated payload");
    Bytes result(data.begin()+at,data.begin()+at+count);at+=count;return result;
}
static uint16_t word(const Bytes& data,size_t& at) {
    const auto value=static_cast<uint16_t>(number(data,at,2));at+=2;return value;
}
struct Operator {uint32_t code;uint8_t precedence;std::string spelling;};
static std::vector<Operator> operators(const SourceImage& source,std::vector<std::string>& rows) {
    std::vector<Operator> result;
    for(uint32_t index=0;index<16;++index) {
        const uint32_t at=0x0118cad0+index*8;
        std::string spelling;std::vector<std::string> raw;bool terminated=false;
        for(uint32_t j=0;j<3;++j) {
            const auto byte=source.number(at+5+j,1);raw.push_back(std::to_string(byte));
            if(!byte)terminated=true;else if(!terminated)spelling+=static_cast<char>(byte);
        }
        if(!terminated||spelling.empty())throw std::runtime_error("operator spelling");
        const auto code=source.number(at,4),precedence=source.number(at+4,1);
        result.push_back({code,static_cast<uint8_t>(precedence),spelling});
        rows.push_back(object({{"descriptor_file_offset",std::to_string(source.offset(at,8))},
            {"table_index",std::to_string(index)},{"code",std::to_string(code)},
            {"precedence",std::to_string(precedence)},{"spelling",quote_json(spelling)},
            {"raw_spelling_bytes",array(raw)}}));
    }
    return result;
}
struct Token {
    size_t start=0,end=0;
    uint8_t tag=0,type=0,precedence=0;
    uint16_t index=0,opcode=0;
    std::optional<uint16_t> context;
    uint32_t operator_code=0;
    std::optional<Bytes> payload;
};
static Bytes token_tuple(const Token& token) {
    Bytes out;
    append(out,static_cast<uint32_t>(token.start),4);append(out,static_cast<uint32_t>(token.end),4);
    append(out,token.tag,1);append(out,token.type,1);append(out,token.index,2);
    append(out,token.context.has_value(),1);append(out,token.context.value_or(0),2);
    append(out,token.operator_code,4);append(out,token.precedence,1);append(out,token.opcode,2);
    append(out,token.payload?static_cast<uint32_t>(token.payload->size()):0,4);
    if(token.payload) {
        const auto hex=digest(*token.payload);
        for(size_t i=0;i<hex.size();i+=2)out.push_back(static_cast<unsigned char>(std::stoul(hex.substr(i,2),nullptr,16)));
    }else out.resize(out.size()+32,0);
    if(out.size()!=58)throw std::runtime_error("token tuple invariant");return out;
}
struct Expression {
    size_t tokens=0;
    Bytes tuples;
    std::optional<uint16_t> pending;
    std::map<uint16_t,uint64_t> commands;
    std::map<uint32_t,uint64_t> operator_codes;
    std::map<uint8_t,uint64_t> kinds;
};
static Expression expression(const Bytes& data,const std::vector<Operator>& ops) {
    if(data.size()>65535)throw std::runtime_error("expression byte budget");
    Expression result;
    for(size_t at=0;at<data.size();) {
        if(data[at]<=0x20){++at;continue;}
        if(result.tokens>=65536)throw std::runtime_error("token budget");
        Token token;token.start=at;const auto byte=data[at++];
        if(byte=='s'||byte=='l'||byte=='f') {
            token.tag=1;token.type=byte;token.index=word(data,at);token.context=result.pending;result.pending.reset();
        }else if(byte=='G'||byte=='Z'||byte=='r') {
            token.tag=byte=='G'?2:byte=='Z'?3:4;token.index=word(data,at);
            if(byte=='r')result.pending=token.index;
        }else if(byte=='X') {
            token.tag=5;token.opcode=word(data,at);const auto length=word(data,at);
            token.payload=take(data,at,length);token.context=result.pending;result.pending.reset();
            ++result.commands[token.opcode];
        }else if(byte=='"') {
            token.tag=6;const auto length=word(data,at);
            if(length>512)throw std::runtime_error("literal budget");token.payload=take(data,at,length);
        }else if(byte=='n'||byte=='z')throw std::runtime_error("unsupported expression token n/z");
        else {
            const Operator* match=nullptr;
            for(const auto& op:ops) {
                if(op.spelling.size()<=data.size()-token.start
                    &&std::equal(op.spelling.begin(),op.spelling.end(),data.begin()+token.start)
                    &&(!match||op.spelling.size()>match->spelling.size()))match=&op;
            }
            if(match) {
                token.tag=8;token.operator_code=match->code;token.precedence=match->precedence;
                at=token.start+match->spelling.size();++result.operator_codes[token.operator_code];
            }else {
                if(!((byte>='0'&&byte<='9')||byte=='.'))throw std::runtime_error("unsupported expression byte");
                const std::string text(data.begin()+token.start,data.end());
                if(text.size()>1&&text[0]=='0'&&(text[1]=='x'||text[1]=='X'))throw std::runtime_error("hexadecimal literal outside decimal subset");
                char* end=nullptr;
                // CRT conversion is used only to obtain an independent decimal
                // boundary. Its value/rounding does not enter the comparison.
                (void)std::strtod(text.c_str(),&end);
                const size_t length=static_cast<size_t>(end-text.c_str());
                if(!length||length>512)throw std::runtime_error("numeric token or budget");
                token.tag=7;at=token.start;token.payload=take(data,at,length);
            }
        }
        token.end=at;++result.tokens;++result.kinds[token.tag];
        const auto tuple=token_tuple(token);result.tuples.insert(result.tuples.end(),tuple.begin(),tuple.end());
    }
    return result;
}
template<class T> static std::string counts(const std::map<T,uint64_t>& data) {
    Object result;for(const auto& entry:data)result.emplace(std::to_string(entry.first),std::to_string(entry.second));return object(result);
}
template<class T> static void merge(std::map<T,uint64_t>& into,const std::map<T,uint64_t>& from) {
    for(const auto& entry:from)into[entry.first]+=entry.second;
}
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
