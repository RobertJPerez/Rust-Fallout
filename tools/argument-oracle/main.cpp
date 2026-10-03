// Original offline native-operand comparison. No original handler is called.
// Signatures and parsing conventions come directly from the fingerprinted PE.
#include "../oracle-common/native_source.hpp"

struct Totals {
    uint64_t top = 0, expressions = 0, operand_bytes = 0, arguments = 0, messages = 0, tail = 0;
    std::map<uint16_t, uint64_t> commands;
    std::map<uint8_t, uint64_t> kinds;
    std::string json() const {
        return object({{"top_level_calls", std::to_string(top)}, {"expression_calls", std::to_string(expressions)},
            {"operand_bytes", std::to_string(operand_bytes)}, {"arguments", std::to_string(arguments)},
            {"message_arguments", std::to_string(messages)}, {"trailing_operand_bytes", std::to_string(tail)},
            {"command_calls", counts(commands)}, {"value_kinds", counts(kinds)}});
    }
};

static std::string call(size_t instruction, std::optional<size_t> token, size_t offset,
    uint16_t opcode, std::optional<uint16_t> context, const Bytes& bytes,
    const std::vector<Signature>& catalogue, Totals& totals, size_t& rows) {
    if (++rows > 1000000) throw std::runtime_error("call row budget");
    if (bytes.size() > 65535 || opcode < 0x1000 || opcode - 0x1000 >= catalogue.size())
        throw std::runtime_error("argument byte or signature range");
    const auto& signature = catalogue[opcode - 0x1000];
    const bool message = message_parser(signature.parse);
    if (!message && signature.parse != 0x005b1ba0) throw std::runtime_error("unverified parser");
    size_t minimum = 0;
    for (size_t i = 0; i < signature.parameters.size(); ++i) {
        const auto optional = signature.parameters[i].optional;
        if (!optional) minimum = i + 1;
        else if (optional != 1) throw std::runtime_error("optional word");
    }
    size_t at = 0;
    std::optional<uint16_t> declared, message_count;
    if (!bytes.empty()) declared = word(bytes, at);
    const auto count = declared.value_or(0);
    if (count > 0x7fff || count < minimum || count > signature.parameters.size())
        throw std::runtime_error("argument count");
    Bytes ordinary, substitutions;
    for (size_t i = 0; i < count; ++i) {
        const size_t start = at;
        auto value = operand(bytes, at, signature.parameters[i].type);
        value.start = start; value.end = at;
        const auto tuple = value_tuple(value, signature.parameters[i].type);
        ordinary.insert(ordinary.end(), tuple.begin(), tuple.end());
        ++totals.kinds[value.tag];
    }
    if (message && at < bytes.size()) {
        message_count = word(bytes, at);
        if (*message_count > 9 || count + *message_count > 64) throw std::runtime_error("message count");
        for (uint16_t i = 0; i < *message_count; ++i) {
            const size_t start = at;
            auto value = numeric(bytes, at); value.start = start; value.end = at;
            const auto tuple = value_tuple(value, UINT32_MAX);
            substitutions.insert(substitutions.end(), tuple.begin(), tuple.end());
            ++totals.kinds[value.tag];
        }
    }
    const auto tail = take(bytes, at, bytes.size() - at);
    totals.top += !token.has_value(); totals.expressions += token.has_value();
    totals.operand_bytes += bytes.size(); totals.arguments += count;
    totals.messages += message_count.value_or(0); totals.tail += tail.size(); ++totals.commands[opcode];
    return object({{"instruction_scda_offset", std::to_string(instruction)},
        {"expression_token_offset", token ? std::to_string(*token) : "null"},
        {"arguments_scda_offset", std::to_string(offset)}, {"command_id", std::to_string(opcode)},
        {"calling_reference", context ? std::to_string(*context) : "null"},
        {"operand_bytes", std::to_string(bytes.size())}, {"operand_sha256", quote_json(digest(bytes))},
        {"declared_count", declared ? std::to_string(*declared) : "null"},
        {"arguments", std::to_string(count)}, {"argument_sha256", quote_json(digest(ordinary))},
        {"declared_message_count", message_count ? std::to_string(*message_count) : "null"},
        {"message_arguments", std::to_string(message_count.value_or(0))},
        {"message_argument_sha256", quote_json(digest(substitutions))},
        {"trailing_operand_bytes", std::to_string(tail.size())},
        {"trailing_operand_sha256", quote_json(digest(tail))}, {"issue", "null"}});
}

static std::string body(const Bytes& bytes, const std::vector<Operator>& ops,
    const std::vector<Signature>& catalogue, size_t& rows) {
    std::vector<std::string> calls;
    Totals totals;
    size_t instructions = 0;
    for (size_t at = 0; at < bytes.size();) {
        if (++instructions > 262144) throw std::runtime_error("instruction budget");
        const size_t start = at;
        auto opcode = word(bytes, at);
        std::optional<uint16_t> context;
        if (opcode == 0x1c) { context = word(bytes, at); opcode = word(bytes, at); }
        const auto length = word(bytes, at);
        const size_t operand_offset = at;
        const auto operands = take(bytes, at, length);
        if (opcode == 0x10 && length < 6) throw std::runtime_error("event header");
        if (opcode >= 0x1000) {
            calls.push_back(call(start, {}, operand_offset, opcode, context, operands, catalogue, totals, rows));
        } else if (opcode == 0x15 || opcode == 0x16 || opcode == 0x18) {
            size_t cursor = 0;
            if (opcode != 0x15) (void)word(operands, cursor);
            else {
                auto target = number(operands, cursor++, 1);
                bool foreign = false;
                if (target == 'r') {
                    foreign = true; (void)word(operands, cursor); target = number(operands, cursor++, 1);
                }
                if (target != 'f' && target != 's' && !(target == 'G' && !foreign))
                    throw std::runtime_error("assignment target");
                (void)word(operands, cursor);
            }
            const auto size = word(operands, cursor);
            const size_t expression_offset = cursor;
            const auto decoded = expression(take(operands, cursor, size), ops);
            for (const auto& token : decoded.decoded_tokens) if (token.tag == 5) {
                calls.push_back(call(start, token.start, operand_offset + expression_offset + token.start + 5,
                    token.opcode, token.context, *token.payload, catalogue, totals, rows));
            }
        }
    }
    return object({{"bytes", std::to_string(bytes.size())}, {"sha256", quote_json(digest(bytes))},
        {"calls", array(calls)}, {"expression_issues", "[]"}, {"counts", totals.json()}});
}

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 3) throw std::runtime_error("usage: argument-oracle comparison-bundle FalloutNV.exe");
        setlocale(LC_NUMERIC, "C");
        const SourceImage source(argv[2]);
        std::ifstream input(argv[1], std::ios::binary | std::ios::ate);
        const auto size = input.tellg();
        if (!input || size < 8 || size > 66 * 1024 * 1024) throw std::runtime_error("bundle byte budget");
        Bytes bundle(static_cast<size_t>(size)); input.seekg(0);
        input.read(reinterpret_cast<char*>(bundle.data()), size);
        if (!input || std::string(bundle.begin(), bundle.begin() + 8) != "FROBS001")
            throw std::runtime_error("bundle magic or read");
        std::vector<std::string> operator_rows, bodies;
        const auto ops = operators(source, operator_rows);
        const auto catalogue = signatures(source);
        size_t rows = 0;
        for (size_t at = 8; at < bundle.size();) {
            if (bodies.size() >= 65536) throw std::runtime_error("body row budget");
            const auto length = number(bundle, at, 4); at += 4;
            if (length > 4 * 1024 * 1024) throw std::runtime_error("compiled byte budget");
            bodies.push_back(body(take(bundle, at, length), ops, catalogue, rows));
        }
        std::cout << object({{"schema_version", "1"}, {"bundle_sha256", quote_json(digest(bundle))},
            {"executable_source_sha256", quote_json(source.sha256)}, {"operator_descriptors", array(operator_rows)},
            {"compiled_bodies", std::to_string(bodies.size())}, {"bodies", array(bodies)},
            {"execution_ready", "false"}}) << '\n';
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "argument-oracle: " << error.what() << '\n'; return 1;
    }
}
