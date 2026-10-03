// Owned offline structural oracle. Token decoding reuses our earlier independent
// reader. Tree construction scans backward with pending child slots, independently
// of the Rust planner's forward operand stack. Neither reader evaluates a value.
#include "../oracle-common/expression_source.hpp"

struct Node {
    size_t token = 0, start = 0, height = 0;
    unsigned arity = 0;
    uint32_t left = UINT32_MAX, right = UINT32_MAX;
};
struct Shape {
    Bytes tuples;
    size_t nodes = 0, maximum_stack = 0, height = 0;
};
struct ShapeError : std::runtime_error {
    std::string diagnostic;
    ShapeError(const std::string &kind, std::optional<size_t> offset = {},
               std::optional<uint32_t> code = {}, std::optional<size_t> needed = {},
               std::optional<size_t> available = {})
        : std::runtime_error(kind) {
        diagnostic =
            object({{"kind", quote_json(kind)},
                    {"expression_byte_offset", offset ? std::to_string(*offset) : "null"},
                    {"operator_code", code ? std::to_string(*code) : "null"},
                    {"needed_operands", needed ? std::to_string(*needed) : "null"},
                    {"available_operands", available ? std::to_string(*available) : "null"},
                    {"budget", "null"}});
    }
};
static Shape shape(const Expression &expression) {
    std::vector<Node> nodes;
    std::optional<size_t> prefix;
    size_t balance = 0;
    for (size_t index = 0; index < expression.decoded_tokens.size(); ++index) {
        const auto &token = expression.decoded_tokens[index];
        if (token.tag == 4) {
            if (prefix)
                throw ShapeError("context", expression.decoded_tokens[*prefix].start);
            prefix = index;
            continue;
        }
        if (token.tag == 1 || token.tag == 5)
            prefix.reset();
        Node node;
        node.token = index;
        if (token.tag == 8) {
            if (token.operator_code >= 2 && token.operator_code <= 14)
                node.arity = 2;
            else if (token.operator_code == 15)
                node.arity = 1;
            else
                throw ShapeError("operator", token.start, token.operator_code);
            if (balance < node.arity)
                throw ShapeError("underflow", token.start, token.operator_code, node.arity,
                                 balance);
            balance -= node.arity - 1;
        } else
            ++balance;
        if (nodes.size() >= 65536)
            throw std::runtime_error("node budget");
        nodes.push_back(node);
    }
    if (prefix)
        throw ShapeError("context", expression.decoded_tokens[*prefix].start);
    if (balance != 1)
        throw ShapeError("residual", {}, {}, 1, balance);
    struct Slot {
        uint32_t parent;
        bool right;
    };
    std::vector<Slot> slots{{UINT32_MAX, false}};
    for (size_t remaining = nodes.size(); remaining; --remaining) {
        if (slots.empty())
            throw std::runtime_error("extra expression operand");
        const auto index = static_cast<uint32_t>(remaining - 1);
        const auto slot = slots.back();
        slots.pop_back();
        if (slot.parent != UINT32_MAX) {
            if (slot.right)
                nodes[slot.parent].right = index;
            else
                nodes[slot.parent].left = index;
        }
        // Reverse postfix visits the right subtree before the left subtree.
        if (nodes[index].arity)
            slots.push_back({index, false});
        if (nodes[index].arity == 2)
            slots.push_back({index, true});
    }
    if (!slots.empty())
        throw std::runtime_error("missing expression operand");
    Shape result;
    size_t depth = 0;
    for (size_t index = 0; index < nodes.size(); ++index) {
        auto &node = nodes[index];
        node.start = index;
        node.height = 1;
        if (!node.arity)
            ++depth;
        else {
            if (node.left >= index)
                throw std::runtime_error("child index invariant");
            node.start = nodes[node.left].start;
            node.height = nodes[node.left].height + 1;
            if (node.arity == 2) {
                if (node.right >= index || node.left >= node.right || depth < 2)
                    throw std::runtime_error("binary child invariant");
                node.height = std::max(node.height, nodes[node.right].height + 1);
                --depth;
            }
        }
        if (depth > 65536 || node.height > 65536)
            throw std::runtime_error("structural budget");
        result.maximum_stack = std::max(result.maximum_stack, depth);
        const auto &token = expression.decoded_tokens[node.token];
        append(result.tuples, static_cast<uint32_t>(node.token), 4);
        append(result.tuples, static_cast<uint32_t>(token.start), 4);
        append(result.tuples, static_cast<uint32_t>(token.end), 4);
        append(result.tuples, node.arity, 1);
        append(result.tuples, token.tag == 8 ? token.operator_code : 0, 4);
        append(result.tuples, node.left, 4);
        append(result.tuples, node.right, 4);
        append(result.tuples, static_cast<uint32_t>(node.start), 4);
        append(result.tuples, static_cast<uint32_t>(node.height), 4);
    }
    if (depth != 1 || result.tuples.size() != nodes.size() * 33)
        throw std::runtime_error("shape tuple invariant");
    result.nodes = nodes.size();
    result.height = nodes.back().height;
    return result;
}
struct Totals {
    size_t statements = 0, nodes = 0, maximum_stack = 0, maximum_height = 0;
    size_t complete_plans = 0, structural_issues = 0;
};
static std::string body(const Bytes &data, const std::vector<Operator> &ops, Totals &totals,
                        bool diagnostic) {
    std::vector<std::string> statements;
    size_t instructions = 0;
    for (size_t at = 0; at < data.size();) {
        if (++instructions > 262144)
            throw std::runtime_error("instruction budget");
        const auto start = at;
        const bool reference = number(data, at, 2) == 0x1c;
        const size_t header = reference ? 8 : 4;
        const auto opcode = number(data, at + (reference ? 4 : 0), 2);
        const auto length = number(data, at + header - 2, 2);
        at += header;
        const auto operands = take(data, at, length);
        if (opcode == 0x10 && length < 6)
            throw std::runtime_error("short event header");
        if (opcode != 0x15 && opcode != 0x16 && opcode != 0x18)
            continue;
        size_t cursor = 0;
        if (opcode == 0x16 || opcode == 0x18)
            (void)word(operands, cursor);
        else {
            auto target = number(operands, cursor++, 1);
            bool context = false;
            if (target == 'r') {
                (void)word(operands, cursor);
                target = number(operands, cursor++, 1);
                context = true;
            }
            if (!(target == 's' || target == 'f' || (target == 'G' && !context)))
                throw std::runtime_error("unsupported assignment target");
            (void)word(operands, cursor);
        }
        const auto expression_bytes = word(operands, cursor);
        const auto expression_offset = cursor;
        const auto bytes = take(operands, cursor, expression_bytes);
        if (cursor != operands.size())
            throw std::runtime_error("expression statement tail");
        const auto decoded = expression(bytes, ops);
        if (totals.statements >= 262144)
            throw std::runtime_error("aggregate statement budget");
        std::string facts = "null", issue = "null";
        try {
            const auto plan = shape(decoded);
            if (plan.nodes > 2000000 - totals.nodes)
                throw std::runtime_error("aggregate node budget");
            ++totals.complete_plans;
            totals.nodes += plan.nodes;
            totals.maximum_stack = std::max(totals.maximum_stack, plan.maximum_stack);
            totals.maximum_height = std::max(totals.maximum_height, plan.height);
            facts = object({{"shape_sha256", quote_json(digest(plan.tuples))},
                            {"nodes", std::to_string(plan.nodes)},
                            {"root", std::to_string(plan.nodes - 1)},
                            {"maximum_stack", std::to_string(plan.maximum_stack)},
                            {"height", std::to_string(plan.height)}});
        } catch (const ShapeError &error) {
            if (!diagnostic)
                throw;
            ++totals.structural_issues;
            issue = error.diagnostic;
        }
        ++totals.statements;
        statements.push_back(
            object({{"instruction_scda_offset", std::to_string(start)},
                    {"opcode", std::to_string(opcode)},
                    {"expression_operand_offset", std::to_string(expression_offset)},
                    {"expression_sha256", quote_json(digest(bytes))},
                    {"token_sha256", quote_json(digest(decoded.tuples))},
                    {"plan", facts},
                    {"issue", issue}}));
    }
    return object({{"bytes", std::to_string(data.size())},
                   {"sha256", quote_json(digest(data))},
                   {"statements", array(statements)}});
}
static bool compare(const std::filesystem::path &path, const SourceImage &source, bool diagnostic) {
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    const auto size = input.tellg();
    if (!input || size < 8 || size > 66 * 1024 * 1024)
        throw std::runtime_error("bundle byte budget");
    Bytes bundle(static_cast<size_t>(size));
    input.seekg(0);
    input.read(reinterpret_cast<char *>(bundle.data()), size);
    if (!input || std::string(bundle.begin(), bundle.begin() + 8) != "FROBS001")
        throw std::runtime_error("bundle read or magic");
    std::vector<std::string> operator_rows, bodies;
    const auto ops = operators(source, operator_rows);
    const std::vector<std::string> spellings{"(",  ")",  "&&", "||", "<=", "<", ">=", ">",
                                             "==", "!=", "-",  "+",  "*",  "/", "%",  "~"};
    std::vector<bool> seen(16, false);
    for (const auto &op : ops) {
        if (op.code >= spellings.size() || seen[op.code] || op.spelling != spellings[op.code])
            throw std::runtime_error("operator model mismatch");
        seen[op.code] = true;
    }
    Totals totals;
    for (size_t at = 8; at < bundle.size();) {
        if (bodies.size() >= 65536)
            throw std::runtime_error("body count budget");
        const auto length = number(bundle, at, 4);
        at += 4;
        if (length > 4 * 1024 * 1024)
            throw std::runtime_error("body byte budget");
        bodies.push_back(body(take(bundle, at, length), ops, totals, diagnostic));
    }
    std::cout
        << object(
               {{"schema_version", "1"},
                {"bundle_sha256", quote_json(digest(bundle))},
                {"executable_source_sha256", quote_json(source.sha256)},
                {"operator_descriptors", array(operator_rows)},
                {"plans",
                 object({{"schema_version", "1"},
                         {"bundle_sha256", quote_json(digest(bundle))},
                         {"compiled_bodies", std::to_string(bodies.size())},
                         {"bodies", array(bodies)},
                         {"counts",
                          object({{"statements", std::to_string(totals.statements)},
                                  {"nodes", std::to_string(totals.nodes)},
                                  {"complete_plans", std::to_string(totals.complete_plans)},
                                  {"structural_issues", std::to_string(totals.structural_issues)},
                                  {"maximum_stack", std::to_string(totals.maximum_stack)},
                                  {"maximum_height", std::to_string(totals.maximum_height)}})},
                         {"execution_ready", "false"}})},
                {"execution_ready", "false"}})
        << '\n';
    return totals.structural_issues != 0;
}
int wmain(int argc, wchar_t **argv) {
    try {
        if (argc != 3 && argc != 4)
            throw std::runtime_error("usage: expression-plan-oracle comparison-bundle "
                                     "FalloutNV.exe [--diagnose-structure]");
        const bool diagnostic = argc == 4 && std::wstring(argv[3]) == L"--diagnose-structure";
        if (argc == 4 && !diagnostic)
            throw std::runtime_error("unknown option");
        const SourceImage source(argv[2]);
        return compare(argv[1], source, diagnostic) ? 1 : 0;
    } catch (const std::exception &error) {
        std::cerr << "expression-plan-oracle: " << error.what() << '\n';
        return 1;
    }
}
