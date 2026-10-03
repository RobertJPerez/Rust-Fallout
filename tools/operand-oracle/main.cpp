// Original offline association reader. Authored tables are not loaded objects.
#include "../oracle-common/native_source.hpp"
#include "../oracle-common/table_source.hpp"

struct Use {
    size_t offset = 0;
    uint8_t role = 0, status = 0, context_kind = 0;
    uint16_t index = 0;
    std::optional<uint16_t> context;
    std::optional<uint32_t> target;
    std::optional<size_t> reference_field, declaration;
    std::optional<uint8_t> type;
    uint32_t context_value = 0;
};

static Use reference(const fallout_tables::Summary& unit, size_t offset, uint8_t role, uint16_t index) {
    Use result; result.offset = offset; result.role = role; result.index = index;
    if (!index || index > unit.references.size()) { result.status = 6; return result; }
    const auto& entry = unit.references[index - 1];
    result.reference_field = entry.offset; result.target = entry.target;
    if (!entry.kind) result.status = 2;
    else {
        const auto local = unit.variables.find(entry.target);
        if (local == unit.variables.end()) result.status = 7;
        else {
            result.status = 3; result.declaration = local->second.offset;
            result.type = local->second.declaration[16];
        }
    }
    return result;
}

static Use local(const fallout_tables::Summary& unit, size_t offset, uint8_t role,
    uint16_t index, std::optional<uint16_t> context) {
    Use result; result.offset = offset; result.role = role; result.index = index; result.context = context;
    if (context) {
        const auto bound = reference(unit, offset, role, *context);
        result.status = bound.status < 5 ? 4 : bound.status;
        result.target = index; result.reference_field = bound.reference_field;
        result.context_kind = bound.status == 2 ? 1 : bound.status == 3 ? 2 : 0;
        result.context_value = bound.target.value_or(0);
    } else {
        const auto variable = unit.variables.find(index);
        if (variable == unit.variables.end()) result.status = 5;
        else {
            result.status = 1; result.target = index; result.declaration = variable->second.offset;
            result.type = variable->second.declaration[16];
        }
    }
    return result;
}

struct Binding {
    Bytes tuples;
    uint64_t uses = 0, calls = 0, expression_calls = 0, arguments = 0, messages = 0,
        expressions = 0, foreign = 0;
    std::map<uint8_t, uint64_t> roles, statuses;
    void push(const Use& use) {
        if (uses >= 262144) throw std::runtime_error("operand use budget");
        if (use.status >= 5) throw std::runtime_error("missing operand binding");
        Bytes tuple;
        append(tuple, static_cast<uint32_t>(use.offset), 4); append(tuple, use.role, 1); append(tuple, use.index, 2);
        append(tuple, use.context.has_value(), 1); append(tuple, use.context.value_or(0), 2);
        append(tuple, use.status, 1); append(tuple, use.target.has_value(), 1); append(tuple, use.target.value_or(0), 4);
        append(tuple, use.reference_field.has_value(), 1); append(tuple, static_cast<uint32_t>(use.reference_field.value_or(0)), 4);
        append(tuple, use.declaration.has_value(), 1); append(tuple, static_cast<uint32_t>(use.declaration.value_or(0)), 4);
        append(tuple, use.type.has_value(), 1); append(tuple, use.type.value_or(0), 1);
        append(tuple, use.context_kind, 1); append(tuple, use.context_value, 4);
        if (tuple.size() != 33) throw std::runtime_error("binding tuple invariant");
        tuples.insert(tuples.end(), tuple.begin(), tuple.end());
        ++uses; ++roles[use.role]; ++statuses[use.status]; foreign += use.status == 4;
    }
    void value(const fallout_tables::Summary& unit, size_t offset, const Value& value) {
        if (value.tag == 6) push(reference(unit, offset + 1, 9, value.index));
        else if (value.tag == 8) push(reference(unit, offset + 1, 10, value.index));
        else if (value.tag == 9) push(local(unit, offset + 1, 11, value.index, {}));
        else if (value.tag == 7) {
            if (value.context) push(reference(unit, offset + 1, 12, *value.context));
            push(local(unit, offset + (value.context ? 4 : 1), 8, value.index, value.context));
        }
    }
    void call(const fallout_tables::Summary& unit, const Bytes& bytes, size_t offset,
        uint16_t opcode, const std::vector<Signature>& catalogue) {
        if (opcode < 0x1000 || opcode - 0x1000 >= catalogue.size()) throw std::runtime_error("command range");
        const auto& signature = catalogue[opcode - 0x1000];
        const bool message = message_parser(signature.parse);
        if (!message && signature.parse != 0x005b1ba0) throw std::runtime_error("unverified parser");
        size_t minimum = 0;
        for (size_t i = 0; i < signature.parameters.size(); ++i) {
            if (!signature.parameters[i].optional) minimum = i + 1;
            else if (signature.parameters[i].optional != 1) throw std::runtime_error("optional word");
        }
        size_t cursor = 0;
        const uint16_t count = bytes.empty() ? 0 : word(bytes, cursor);
        if (count > 0x7fff || count < minimum || count > signature.parameters.size()) throw std::runtime_error("argument count");
        arguments += count;
        for (size_t i = 0; i < count; ++i) {
            const size_t start = cursor;
            const auto decoded = operand(bytes, cursor, signature.parameters[i].type);
            value(unit, offset + start, decoded);
        }
        if (message && cursor < bytes.size()) {
            const auto substitutions = word(bytes, cursor);
            if (substitutions > 9 || count + substitutions > 64) throw std::runtime_error("message count");
            messages += substitutions;
            for (uint16_t i = 0; i < substitutions; ++i) {
                const size_t start = cursor; const auto decoded = numeric(bytes, cursor);
                value(unit, offset + start, decoded);
            }
        }
    }
    std::string counts_json() const {
        return object({{"uses", std::to_string(uses)}, {"roles", counts(roles)}, {"statuses", counts(statuses)},
            {"instruction_calls", std::to_string(calls)}, {"expression_calls", std::to_string(expression_calls)},
            {"regular_arguments", std::to_string(arguments)}, {"message_arguments", std::to_string(messages)},
            {"expressions", std::to_string(expressions)}, {"deferred_foreign_locals", std::to_string(foreign)},
            {"missing_bindings", "0"}});
    }
};

static Binding bind(const fallout_tables::Summary& unit, const std::vector<Operator>& ops,
    const std::vector<Signature>& catalogue) {
    const auto& data = *unit.compiled;
    Binding result;
    for (size_t at = 0; at < data.size();) {
        const size_t start = at; auto opcode = word(data, at);
        if (opcode == 0x1c) {
            const auto index = word(data, at);
            result.push(reference(unit, start + 2, 1, index)); opcode = word(data, at);
        }
        const auto length = word(data, at); const size_t offset = at;
        const auto operands = take(data, at, length);
        if (opcode >= 0x1000) {
            ++result.calls; result.call(unit, operands, offset, opcode, catalogue); continue;
        }
        if (opcode != 0x15 && opcode != 0x16 && opcode != 0x18) continue;
        ++result.expressions;
        size_t cursor = 0;
        if (opcode != 0x15) (void)word(operands, cursor);
        else {
            auto target = number(operands, cursor++, 1); std::optional<uint16_t> context;
            if (target == 'r') {
                context = word(operands, cursor); result.push(reference(unit, offset + 1, 7, *context));
                target = number(operands, cursor++, 1);
            }
            const size_t index_offset = cursor; const auto index = word(operands, cursor);
            if (target == 'f' || target == 's') result.push(local(unit, offset + index_offset, 2, index, context));
            else if (target == 'G' && !context) result.push(reference(unit, offset + index_offset, 3, index));
            else throw std::runtime_error("assignment target");
        }
        const auto size = word(operands, cursor); const size_t expression_offset = offset + cursor;
        const auto decoded = expression(take(operands, cursor, size), ops);
        for (const auto& token : decoded.decoded_tokens) {
            const size_t offset = expression_offset + token.start;
            switch (token.tag) {
            case 1: result.push(local(unit, offset + 1, 4, token.index, token.context)); break;
            case 2: result.push(reference(unit, offset + 1, 5, token.index)); break;
            case 3: result.push(reference(unit, offset + 1, 6, token.index)); break;
            case 4: result.push(reference(unit, offset + 1, 7, token.index)); break;
            case 5: ++result.expression_calls; result.call(unit, *token.payload, offset + 5, token.opcode, catalogue); break;
            default: break;
            }
        }
    }
    return result;
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 3) throw std::runtime_error("usage: operand-oracle decoded-record-bundle FalloutNV.exe");
        setlocale(LC_NUMERIC, "C"); const SourceImage source(argv[2]);
        std::vector<std::string> operator_rows; const auto ops = operators(source, operator_rows);
        const auto catalogue = signatures(source);
        std::ifstream input(argv[1], std::ios::binary | std::ios::ate); const auto size = input.tellg();
        if (!input || size < 8 || size > 512 * 1024 * 1024) throw std::runtime_error("bundle byte budget");
        Bytes bundle(static_cast<size_t>(size)); input.seekg(0); input.read(reinterpret_cast<char*>(bundle.data()), size);
        if (!input || std::string(bundle.begin(), bundle.begin() + 8) != "FRUNIT01") throw std::runtime_error("bundle magic/read");
        size_t records = 0; uint64_t uses = 0;
        std::vector<std::string> rows;
        for (size_t at = 8; at < bundle.size();) {
            if (++records > 262144) throw std::runtime_error("record budget");
            const auto signature_bytes = take(bundle, at, 4);
            const std::string kind(signature_bytes.begin(), signature_bytes.end());
            if (!std::all_of(kind.begin(), kind.end(), [](char c) { return (c >= 'A' && c <= 'Z') || c == '_'; }))
                throw std::runtime_error("record signature");
            const auto form = number(bundle, at, 4); at += 4;
            const auto file_offset = fallout_tables::integer(bundle, at, 8); at += 8;
            const auto length = number(bundle, at, 4); at += 4;
            if (length > 64 * 1024 * 1024) throw std::runtime_error("record byte budget");
            const auto scripts = fallout_tables::units(take(bundle, at, length));
            if (scripts.empty()) throw std::runtime_error("record lacks script unit");
            for (const auto& fields : scripts) {
                const auto unit = fallout_tables::inspect(fields);
                if (!unit.compiled) continue;
                if (rows.size() >= 65536) throw std::runtime_error("compiled unit budget");
                const auto binding = bind(unit, ops, catalogue); uses += binding.uses;
                if (uses > 2000000) throw std::runtime_error("run use budget");
                rows.push_back(object({{"record_kind", quote_json(kind)}, {"form_id", std::to_string(form)},
                    {"record_file_offset", std::to_string(file_offset)}, {"header_decoded_offset", std::to_string(fields.front().offset)},
                    {"metadata_sha256", quote_json(digest(unit.metadata))}, {"compiled_bytes", std::to_string(unit.compiled->size())},
                    {"compiled_sha256", quote_json(digest(*unit.compiled))}, {"binding_sha256", quote_json(digest(binding.tuples))},
                    {"counts", binding.counts_json()}, {"decode_issues", "[]"}, {"missing_bindings", "[]"}}));
            }
        }
        std::cout << object({{"schema_version", "1"}, {"bundle_sha256", quote_json(digest(bundle))},
            {"executable_source_sha256", quote_json(source.sha256)}, {"compiled_units", array(rows)},
            {"compiled_unit_count", std::to_string(rows.size())}, {"execution_ready", "false"}}) << '\n';
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "operand-oracle: " << error.what() << '\n'; return 1;
    }
}
