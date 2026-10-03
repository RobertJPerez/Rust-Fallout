// Original offline expression reader shared by comparison executables.
#pragma once
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
    std::vector<Token> decoded_tokens;
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
        result.decoded_tokens.push_back(std::move(token));
    }
    return result;
}
template<class T> static std::string counts(const std::map<T,uint64_t>& data) {
    Object result;for(const auto& entry:data)result.emplace(std::to_string(entry.first),std::to_string(entry.second));return object(result);
}
template<class T> static void merge(std::map<T,uint64_t>& into,const std::map<T,uint64_t>& from) {
    for(const auto& entry:from)into[entry.first]+=entry.second;
}
