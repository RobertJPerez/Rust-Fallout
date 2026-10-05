// Offline reference exporter only; never linked into the Rust runtime.
#include <Pex/FileReader.hpp>
#include <filesystem>
#include <iostream>
#include <map>
#include <algorithm>
#include <vector>
#include <sstream>
#include "values.hpp"

static std::string quote(const std::string& s) {
    std::ostringstream out; out << '"';
    const char* hex = "0123456789abcdef";
    for (unsigned char c : s) {
        if (c == '"' || c == '\\') out << '\\' << c;
        else if (c < 32 || c >= 127) out << "\\u00" << hex[c >> 4] << hex[c & 15];
        else out << c;
    }
    out << '"'; return out.str();
}

struct Stats {
    size_t structs=0, variables=0, properties=0, states=0, functions=0, instructions=0;
    std::map<unsigned,size_t> opcodes;
    std::vector<std::string> natives;
    void function(const Pex::Object& object, const Pex::Function& f,
                  const std::string& name, const std::string* state, const char* kind) {
        ++functions; instructions += f.getInstructions().size();
        for (const auto& i : f.getInstructions()) ++opcodes[static_cast<unsigned>(i.getOpCode())];
        if (!f.isNative()) return;
        std::ostringstream out;
        out << "{\"class\":" << quote(object.getName().asString())
            << ",\"parent\":" << quote(object.getParentClassName().asString())
            << ",\"state\":" << (state ? quote(*state) : "null")
            << ",\"function\":" << quote(name) << ",\"kind\":" << quote(kind)
            << ",\"flags\":" << (2 + (f.isGlobal() ? 1 : 0))
            << ",\"return_type\":" << quote(f.getReturnTypeName().asString())
            << ",\"parameters\":[";
        bool first=true;
        for (const auto& p : f.getParams()) {
            if (!first) out << ','; first=false;
            out << '[' << quote(p.getName().asString()) << ',' << quote(p.getTypeName().asString()) << ']';
        }
        out << "]}"; natives.push_back(out.str());
    }
};

int main(int argc,char** argv) {
    if (argc!=2) { std::cerr << "usage: fo4-pex-oracle <extracted-directory>\n"; return 1; }
    std::vector<std::filesystem::path> paths;
    for (const auto& entry:std::filesystem::directory_iterator(argv[1]))
        if (entry.is_regular_file() && entry.path().extension()==".pex") paths.push_back(entry.path());
    std::sort(paths.begin(),paths.end());
    bool failed=false;
    for (const auto& path:paths) {
        try {
            std::ifstream stream(path,std::ios::binary);
            Pex::FileReader reader(&stream); Pex::Binary binary; reader.read(binary);
            if (stream.tellg()!=static_cast<std::streamoff>(std::filesystem::file_size(path)))
                throw std::runtime_error("unconsumed bytes");
            Stats s;
            for (const auto& object:binary.getObjects()) {
                s.structs += object.getStructInfos().size(); s.variables += object.getVariables().size();
                s.properties += object.getProperties().size(); s.states += object.getStates().size();
                for (const auto& p:object.getProperties()) {
                    if (p.hasAutoVar()) continue;
                    if (p.isReadable()) s.function(object,p.getReadFunction(),p.getName().asString(),nullptr,"getter");
                    if (p.isWritable()) s.function(object,p.getWriteFunction(),p.getName().asString(),nullptr,"setter");
                }
                for (const auto& state:object.getStates()) {
                    auto stateName=state.getName().asString();
                    for (const auto& f:state.getFunctions()) s.function(object,f,f.getName().asString(),&stateName,"method");
                }
            }
            std::cout << "{\"file\":" << quote(path.filename().string())
                << ",\"strings\":" << binary.getStringTable().size()
                << ",\"objects\":" << binary.getObjects().size()
                << ",\"structs\":" << s.structs << ",\"variables\":" << s.variables
                << ",\"properties\":" << s.properties << ",\"states\":" << s.states
                << ",\"functions\":" << s.functions << ",\"instructions\":" << s.instructions
                << ",\"opcode_counts\":{";
            bool first=true;
            for (const auto& [opcode,count]:s.opcodes) { if (!first) std::cout << ','; first=false; std::cout << quote(std::to_string(opcode)) << ':' << count; }
            std::cout << "},\"native_declarations\":["; first=true;
            for (const auto& n:s.natives) { if (!first) std::cout << ','; first=false; std::cout << n; }
            std::cout << "],\"value_tokens\":[";
            ValueTokens values{std::cout}; values.binary(binary);
            std::cout << "]}\n";
        } catch(const std::exception& e) {
            failed=true;
            std::cout << "{\"file\":" << quote(path.filename().string()) << ",\"error\":" << quote(e.what()) << "}\n";
        }
    }
    return failed ? 2 : 0;
}
