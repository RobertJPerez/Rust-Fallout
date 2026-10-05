// Export upstream typed values in wire order for independent Rust comparison.
#pragma once
#include <cstring>
#include <ostream>

struct ValueTokens {
    std::ostream& out;
    bool first = true;
    void separator() { if (!first) out << ','; first = false; }
    template<class T> void number(T value) { separator(); out << +value; }
    void bytes(const std::string& value) {
        static const char* hex = "0123456789abcdef";
        separator(); out << '"';
        for (unsigned char c : value) out << hex[c >> 4] << hex[c & 15];
        out << '"';
    }
    void value(const Pex::Value& v) {
        number(static_cast<unsigned>(v.getType()));
        switch (v.getType()) {
        case Pex::ValueType::None: break;
        case Pex::ValueType::Identifier: number(v.getId().asIndex()); break;
        case Pex::ValueType::String: number(v.getString().asIndex()); break;
        case Pex::ValueType::Integer: number(v.getInteger()); break;
        case Pex::ValueType::Float: {
            float f = v.getFloat(); std::uint32_t bits;
            static_assert(sizeof(f) == sizeof(bits)); std::memcpy(&bits, &f, sizeof(bits));
            number(bits); break;
        }
        case Pex::ValueType::Bool: number(v.getBool() ? 1 : 0); break;
        default: throw std::runtime_error("unhandled upstream operand type");
        }
    }
    void function(const Pex::Function& f) {
        number(f.getReturnTypeName().asIndex()); number(f.getDocString().asIndex());
        number(f.getUserFlags()); number((f.isNative() ? 2 : 0) | (f.isGlobal() ? 1 : 0));
        for (const auto* list : { &f.getParams(), &f.getLocals() }) {
            number(list->size());
            for (const auto& p : *list) { number(p.getName().asIndex()); number(p.getTypeName().asIndex()); }
        }
        number(f.getInstructions().size());
        for (const auto& i : f.getInstructions()) {
            number(static_cast<unsigned>(i.getOpCode())); number(i.getArgs().size());
            for (const auto& a : i.getArgs()) value(a);
            number(i.getVarArgs().size());
            for (const auto& a : i.getVarArgs()) value(a);
        }
    }
    void binary(const Pex::Binary& b) {
        number(b.getStringTable().size());
        for (const auto& s : b.getStringTable()) bytes(s);
        number(b.getUserFlags().size());
        for (const auto& f : b.getUserFlags()) { number(f.getName().asIndex()); number(f.getFlagIndex()); }
        number(b.getObjects().size());
        for (const auto& o : b.getObjects()) {
            number(o.getName().asIndex()); number(o.getParentClassName().asIndex());
            number(o.getDocString().asIndex()); number(o.getConstFlag());
            number(o.getUserFlags()); number(o.getAutoStateName().asIndex());
            number(o.getStructInfos().size());
            for (const auto& s : o.getStructInfos()) {
                number(s.getName().asIndex()); number(s.getMembers().size());
                for (const auto& m : s.getMembers()) {
                    number(m.getName().asIndex()); number(m.getTypeName().asIndex()); number(m.getUserFlags());
                    value(m.getValue()); number(m.getConstFlag()); number(m.getDocString().asIndex());
                }
            }
            number(o.getVariables().size());
            for (const auto& v : o.getVariables()) {
                number(v.getName().asIndex()); number(v.getTypeName().asIndex()); number(v.getUserFlags());
                value(v.getDefaultValue()); number(v.getConstFlag());
            }
            number(o.getProperties().size());
            for (const auto& p : o.getProperties()) {
                number(p.getName().asIndex()); number(p.getTypeName().asIndex());
                number(p.getDocString().asIndex()); number(p.getUserFlags());
                number((p.isReadable() ? 1 : 0) | (p.isWritable() ? 2 : 0) | (p.hasAutoVar() ? 4 : 0));
                if (p.hasAutoVar()) number(p.getAutoVarName().asIndex());
                else {
                    if (p.isReadable()) function(p.getReadFunction());
                    if (p.isWritable()) function(p.getWriteFunction());
                }
            }
            number(o.getStates().size());
            for (const auto& s : o.getStates()) {
                number(s.getName().asIndex()); number(s.getFunctions().size());
                for (const auto& f : s.getFunctions()) { number(f.getName().asIndex()); function(f); }
            }
        }
    }
};
