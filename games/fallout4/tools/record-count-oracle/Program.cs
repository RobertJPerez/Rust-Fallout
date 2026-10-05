using System.Security.Cryptography;
using System.Text.Json;
using Mutagen.Bethesda.Fallout4;
using Mutagen.Bethesda.Plugins;

if (args.Length != 3)
    throw new ArgumentException("fo4-record-count-oracle <install-root> <frozen-proof-dir> <new-local-output-dir>");

var installRoot = Path.GetFullPath(args[0]);
var proofDir = Path.GetFullPath(args[1]);
var outputDir = Path.GetFullPath(args[2]);
var localRoot = Path.GetFullPath("local") + Path.DirectorySeparatorChar;
if (!proofDir.StartsWith(localRoot, StringComparison.OrdinalIgnoreCase)
    || !outputDir.StartsWith(localRoot, StringComparison.OrdinalIgnoreCase)
    || outputDir.StartsWith(installRoot, StringComparison.OrdinalIgnoreCase)
    || outputDir.StartsWith(proofDir, StringComparison.OrdinalIgnoreCase))
    throw new InvalidDataException("proof and new oracle output must be separate directories under local/");
if (Directory.Exists(outputDir) || File.Exists(outputDir))
    throw new InvalidDataException("oracle output must be new; choose a fresh local/ directory");

var proofBytes = File.ReadAllBytes(Path.Combine(proofDir, "complete.json"));
using var proofComplete = JsonDocument.Parse(proofBytes);
if (proofComplete.RootElement.GetProperty("status").GetString() != "matched")
    throw new InvalidDataException("frozen proof is incomplete");
var censusPath = Path.Combine(proofDir, "census.json");
var censusBytes = File.ReadAllBytes(censusPath);
var censusSha = Sha256(censusBytes);
if (proofComplete.RootElement.GetProperty("census_sha256").GetString() != censusSha)
    throw new InvalidDataException("frozen proof completion marker does not bind its census");
using var census = JsonDocument.Parse(censusBytes);
var pluginRows = census.RootElement.GetProperty("plugins");
var pluginRow = pluginRows.EnumerateArray().Single(row =>
    string.Equals(row.GetProperty("name").GetString(), "Fallout4.esm", StringComparison.OrdinalIgnoreCase));
var expectedSourceSha = census.RootElement.GetProperty("files").EnumerateArray().Single(row =>
    string.Equals(row.GetProperty("path").GetString(), "Data/Fallout4.esm", StringComparison.OrdinalIgnoreCase));
var sourcePath = Path.Combine(installRoot, "Data", "Fallout4.esm");
if (Path.GetFileName(sourcePath) != "Fallout4.esm"
    || !File.Exists(sourcePath)
    || Directory.Exists(sourcePath)
    || File.GetAttributes(sourcePath).HasFlag(FileAttributes.ReparsePoint))
    throw new InvalidDataException("Fallout4.esm is not a regular direct Data file");
var expectedHash = expectedSourceSha.GetProperty("sha256").GetString()!;
var expectedBytes = expectedSourceSha.GetProperty("bytes").GetUInt64();
var sourceBefore = HashFile(sourcePath);
if (sourceBefore != expectedHash || (ulong)new FileInfo(sourcePath).Length != expectedBytes)
    throw new InvalidDataException("Fallout4.esm differs from its frozen source census");

uint declaredHeaderCount;
uint mutagenCalculatedCount;
long mutagenMajorRecordCount;
using (var mod = Fallout4Mod.CreateFromBinaryOverlay(
    new ModPath(ModKey.FromFileName("Fallout4.esm"), sourcePath), Fallout4Release.Fallout4))
{
    declaredHeaderCount = mod.ModHeader.Stats.NumRecords;
    mutagenMajorRecordCount = mod.EnumerateMajorRecords().LongCount();
    mutagenCalculatedCount = mod.GetRecordCount();
}

var sourceAfter = HashFile(sourcePath);
if (sourceAfter != sourceBefore)
    throw new InvalidDataException("Fallout4.esm changed while the independent reader was running");

var rustRecordCount = pluginRow.GetProperty("records").GetUInt64();
var rustGroupCount = pluginRow.GetProperty("groups").GetUInt64();
var rustPhysicalCountExcludingTes4 = rustRecordCount == 0 ? 0 : rustRecordCount - 1 + rustGroupCount;
var manifest = new
{
    schema = 1,
    status = "complete-independent-fo4-header-count-comparison-not-game-engine-proof",
    plugin = "Fallout4.esm",
    source_census = "local/proof-fo4-002/census.json",
    source_census_sha256 = censusSha,
    source_file_sha256 = sourceBefore,
    source_file_bytes = expectedBytes,
    source_rechecked_after = true,
    counts = new
    {
        hedr_declared_count = declaredHeaderCount,
        pinned_mutagen_calculated_count = mutagenCalculatedCount,
        pinned_mutagen_enumerated_major_records = mutagenMajorRecordCount,
        rust_raw_record_count_including_tes4 = rustRecordCount,
        rust_physical_group_header_count = rustGroupCount,
        rust_records_plus_physical_groups_excluding_tes4 = rustPhysicalCountExcludingTes4,
        header_minus_rust_physical_count = (long)declaredHeaderCount - (long)rustPhysicalCountExcludingTes4,
        mutagen_calculated_count_matches_hedr = mutagenCalculatedCount == declaredHeaderCount,
        mutagen_major_records_match_rust_raw_record_count = (ulong)mutagenMajorRecordCount + 1 == rustRecordCount
    },
    reference = new
    {
        id = "pinned-mutagen-fallout4-record-count-oracle",
        revision = "4f533562ee0c70347d47c1979d5464d42b06ee6b",
        source_tree_modified = false,
        usage = "Fallout4Mod.CreateFromBinaryOverlay and Fallout4Mod.GetRecordCount; isolated research only"
    },
    retail_source_modified = false,
    runtime_ready = false,
    limits = new[]
    {
        "Mutagen's calculated count validates only its own FO4 data-model and writer count convention; it does not prove the game engine's interpretation.",
        "The Rust physical count is records excluding TES4 plus physical GRUP headers; it is a structural inventory, not the HEDR writer formula.",
        "No record bodies were changed and no game was launched."
    }
};

Directory.CreateDirectory(outputDir);
var jsonOptions = new JsonSerializerOptions { WriteIndented = true };
var manifestPath = Path.Combine(outputDir, "manifest.json");
File.WriteAllText(manifestPath, JsonSerializer.Serialize(manifest, jsonOptions) + "\n");
var completion = new
{
    schema = 1,
    manifest_sha256 = HashFile(manifestPath),
    source_file_sha256 = sourceAfter,
    complete = true,
    runtime_ready = false
};
File.WriteAllText(Path.Combine(outputDir, "complete.json"), JsonSerializer.Serialize(completion, jsonOptions) + "\n");
Console.WriteLine(JsonSerializer.Serialize(completion, jsonOptions));

static string HashFile(string path)
{
    using var stream = File.OpenRead(path);
    return Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
}

static string Sha256(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
