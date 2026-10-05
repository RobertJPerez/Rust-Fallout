using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using Mutagen.Bethesda;
using Mutagen.Bethesda.Plugins;
using Mutagen.Bethesda.Strings;
using Mutagen.Bethesda.Strings.DI;

if (args.Length != 2)
    throw new ArgumentException("usage: strings-oracle <rust-audit-directory> <output-directory>");

var inputRoot = Path.GetFullPath(args[0]);
var outputRoot = Path.GetFullPath(args[1]);
Directory.CreateDirectory(outputRoot);
var tablesPath = Path.Combine(inputRoot, "tables.jsonl");
var keysPath = Path.Combine(inputRoot, "keys.jsonl");
var tableRows = File.ReadLines(tablesPath)
    .Where(line => !string.IsNullOrWhiteSpace(line))
    .Select(line => JsonSerializer.Deserialize<TableDto>(line) ?? throw new InvalidDataException("null table row"))
    .OrderBy(row => row.TableId)
    .ToArray();
var tableHash = Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(tablesPath)));
var keyHash = Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(keysPath)));
var keyReader = new StreamReader(keysPath);
var tableComparisons = new List<TableComparison>(tableRows.Length);
var rows = 0;
var keysCompared = 0;
var missingKeys = 0;
var offsetMismatches = 0;
var valueMismatches = 0;
var exceptions = 0;

foreach (var table in tableRows)
{
    var missing = 0;
    var badOffsets = 0;
    var badValues = 0;
    var tableExceptions = 0;
    var rustKeys = 0;
    var referenceCount = 0;
    string? error = null;
    try
    {
        var rawPath = Path.GetFullPath(Path.Combine(inputRoot, table.ExtractedPath));
        if (!rawPath.StartsWith(inputRoot + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("table payload path escapes audit directory");
        if (!StringComparer.Ordinal.Equals(Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(rawPath))), table.ExtractedSha256))
            throw new InvalidDataException("table payload hash differs from Rust manifest");

        var format = table.Extension switch
        {
            "strings" => StringsFileFormat.Normal,
            "dlstrings" or "ilstrings" => StringsFileFormat.LengthPrepended,
            _ => throw new InvalidDataException($"unsupported extension {table.Extension}"),
        };
        var overlay = new StringsLookupOverlay(
            rawPath,
            format,
            MutagenEncoding.Default.GetEncoding(GameRelease.Fallout4, Language.English));
        referenceCount = overlay.Count;
        if (referenceCount != table.Keys)
            error = $"Rust keys {table.Keys} differ from Mutagen index count {referenceCount}";

        for (var i = 0; i < table.Keys; i++)
        {
            var line = keyReader.ReadLine() ?? throw new InvalidDataException("key manifest ended early");
            var key = JsonSerializer.Deserialize<KeyDto>(line) ?? throw new InvalidDataException("null key row");
            if (key.TableId != table.TableId)
                throw new InvalidDataException($"expected table {table.TableId} key row, found table {key.TableId}");
            rustKeys++;
            keysCompared++;
            if (!overlay.TryGetLocation(key.Key, out var location))
            {
                missing++;
                continue;
            }
            if (location != key.Offset)
                badOffsets++;
            try
            {
                var value = overlay.GetStringBytesAtLocation(location);
                if (value.Length != key.ValueBytes
                    || !StringComparer.Ordinal.Equals(Convert.ToHexStringLower(SHA256.HashData(value)), key.ValueSha256))
                    badValues++;
            }
            catch (Exception ex) when (ex is ArgumentException or ArgumentOutOfRangeException or OverflowException)
            {
                tableExceptions++;
            }
        }
    }
    catch (Exception ex)
    {
        error = ex.GetType().Name + ": " + ex.Message;
        tableExceptions++;
        // Keep the key stream aligned even if table initialization fails.
        for (var i = rustKeys; i < table.Keys; i++)
        {
            if (keyReader.ReadLine() is null)
                break;
            keysCompared++;
        }
    }

    rows++;
    missingKeys += missing;
    offsetMismatches += badOffsets;
    valueMismatches += badValues;
    exceptions += tableExceptions;
    tableComparisons.Add(new TableComparison(
        table.TableId,
        table.Archive,
        table.MemberNameBytesHex,
        table.Extension,
        table.Keys,
        referenceCount,
        rustKeys,
        missing,
        badOffsets,
        badValues,
        tableExceptions,
        error));
}

var extraKeyRows = keyReader.ReadLine() is not null;
var allMatch = rows == tableRows.Length
    && !extraKeyRows
    && missingKeys == 0
    && offsetMismatches == 0
    && valueMismatches == 0
    && exceptions == 0
    && tableComparisons.All(row => row.Error is null && row.MutagenCount == row.RustKeyCount);
var comparison = new Comparison(
    1,
    allMatch ? "matched" : "mismatch",
    "4f533562ee0c70347d47c1979d5464d42b06ee6b",
    tableHash,
    keyHash,
    rows,
    keysCompared,
    missingKeys,
    offsetMismatches,
    valueMismatches,
    exceptions,
    extraKeyRows,
    allMatch,
    false,
    tableComparisons);
await File.WriteAllTextAsync(
    Path.Combine(outputRoot, "comparison.json"),
    JsonSerializer.Serialize(comparison, new JsonSerializerOptions { WriteIndented = true }));
Console.WriteLine(JsonSerializer.Serialize(new
{
    comparison.Status,
    comparison.TablesCompared,
    comparison.KeysCompared,
    comparison.MissingKeys,
    comparison.OffsetMismatches,
    comparison.ValueMismatches,
    comparison.Exceptions,
    comparison.AllRowsMatch,
    comparison.RuntimeReady,
}, new JsonSerializerOptions { WriteIndented = true }));
if (!allMatch)
    Environment.ExitCode = 2;

sealed record TableDto
{
    [JsonPropertyName("table_id")] public int TableId { get; init; }
    [JsonPropertyName("archive")] public string Archive { get; init; } = "";
    [JsonPropertyName("member_name_bytes_hex")] public string MemberNameBytesHex { get; init; } = "";
    [JsonPropertyName("extension")] public string Extension { get; init; } = "";
    [JsonPropertyName("extracted_path")] public string ExtractedPath { get; init; } = "";
    [JsonPropertyName("extracted_sha256")] public string ExtractedSha256 { get; init; } = "";
    [JsonPropertyName("keys")] public int Keys { get; init; }
}

sealed record KeyDto
{
    [JsonPropertyName("table_id")] public int TableId { get; init; }
    [JsonPropertyName("key")] public uint Key { get; init; }
    [JsonPropertyName("offset")] public int Offset { get; init; }
    [JsonPropertyName("value_bytes")] public int ValueBytes { get; init; }
    [JsonPropertyName("value_sha256")] public string ValueSha256 { get; init; } = "";
}

sealed record TableComparison(
    [property: JsonPropertyName("table_id")] int TableId,
    [property: JsonPropertyName("archive")] string Archive,
    [property: JsonPropertyName("member_name_bytes_hex")] string MemberNameBytesHex,
    [property: JsonPropertyName("extension")] string Extension,
    [property: JsonPropertyName("rust_key_count")] int RustKeyCount,
    [property: JsonPropertyName("mutagen_count")] int MutagenCount,
    [property: JsonPropertyName("rust_key_rows_read")] int RustKeyRowsRead,
    [property: JsonPropertyName("missing_keys")] int MissingKeys,
    [property: JsonPropertyName("offset_mismatches")] int OffsetMismatches,
    [property: JsonPropertyName("value_mismatches")] int ValueMismatches,
    [property: JsonPropertyName("exceptions")] int Exceptions,
    [property: JsonPropertyName("error")] string? Error);

sealed record Comparison(
    [property: JsonPropertyName("schema")] int Schema,
    [property: JsonPropertyName("status")] string Status,
    [property: JsonPropertyName("mutagen_revision")] string MutagenRevision,
    [property: JsonPropertyName("rust_tables_manifest_sha256")] string RustTablesManifestSha256,
    [property: JsonPropertyName("rust_keys_manifest_sha256")] string RustKeysManifestSha256,
    [property: JsonPropertyName("tables_compared")] int TablesCompared,
    [property: JsonPropertyName("keys_compared")] int KeysCompared,
    [property: JsonPropertyName("missing_keys")] int MissingKeys,
    [property: JsonPropertyName("offset_mismatches")] int OffsetMismatches,
    [property: JsonPropertyName("value_mismatches")] int ValueMismatches,
    [property: JsonPropertyName("exceptions")] int Exceptions,
    [property: JsonPropertyName("extra_key_rows")] bool ExtraKeyRows,
    [property: JsonPropertyName("all_rows_match")] bool AllRowsMatch,
    [property: JsonPropertyName("runtime_ready")] bool RuntimeReady,
    [property: JsonPropertyName("tables")] IReadOnlyList<TableComparison> Tables);
