// Offline comparison harness. The upstream reader is never a runtime dependency.
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Mutagen.Bethesda.Fallout4;
using Mutagen.Bethesda.Plugins;

if (args.Length == 2 && args[0] == "--write-condition-fixture")
{
    WriteConditionFixture(args[1]);
    return 0;
}

if (args.Length != 4)
    throw new ArgumentException("workshop-oracle <install-root> <proof-dir> <rust-audit-dir> <new-local-report-dir>");
VerifyEditorIDJoinFixtures();

var install = Path.GetFullPath(args[0]);
var proofDir = Path.GetFullPath(args[1]);
var rustDir = Path.GetFullPath(args[2]);
var outputDir = Path.GetFullPath(args[3]);
var localRoot = Path.GetFullPath("local") + Path.DirectorySeparatorChar;
if (!proofDir.StartsWith(localRoot, StringComparison.OrdinalIgnoreCase)
    || !rustDir.StartsWith(localRoot, StringComparison.OrdinalIgnoreCase)
    || !outputDir.StartsWith(localRoot, StringComparison.OrdinalIgnoreCase)
    || outputDir.StartsWith(install, StringComparison.OrdinalIgnoreCase)
    || outputDir.StartsWith(proofDir, StringComparison.OrdinalIgnoreCase)
    || outputDir.StartsWith(rustDir, StringComparison.OrdinalIgnoreCase))
    throw new InvalidDataException("proof, audit and new oracle output must remain separate under local/");
if (Directory.Exists(outputDir) || File.Exists(outputDir))
    throw new InvalidDataException("oracle output must be new; choose a fresh local/ directory");

var proofBytes = File.ReadAllBytes(Path.Combine(proofDir, "complete.json"));
using var proofComplete = JsonDocument.Parse(proofBytes);
var censusBytes = File.ReadAllBytes(Path.Combine(proofDir, "census.json"));
var censusHash = Convert.ToHexString(SHA256.HashData(censusBytes)).ToLowerInvariant();
if (proofComplete.RootElement.GetProperty("census_sha256").GetString() != censusHash)
    throw new InvalidDataException("frozen proof completion marker does not bind its census");
var sourceProofHash = Convert.ToHexString(SHA256.HashData(proofBytes)).ToLowerInvariant();

var rustCompleteBytes = File.ReadAllBytes(Path.Combine(rustDir, "complete.json"));
using var rustComplete = JsonDocument.Parse(rustCompleteBytes);
var rustCompleteRoot = rustComplete.RootElement;
if (rustCompleteRoot.GetProperty("source_census_sha256").GetString() != censusHash
    || rustCompleteRoot.GetProperty("source_proof_complete_sha256").GetString() != sourceProofHash)
    throw new InvalidDataException("Rust workshop audit does not match the frozen proof");
var recipesPath = Path.Combine(rustDir, "recipes.jsonl");
using (var recipesStream = File.OpenRead(recipesPath))
{
    var actualHash = Convert.ToHexString(SHA256.HashData(recipesStream)).ToLowerInvariant();
    if (actualHash != rustCompleteRoot.GetProperty("recipes_jsonl_sha256").GetString())
        throw new InvalidDataException("Rust recipe evidence hash differs from its completion marker");
}
var componentEvidencePath = Path.Combine(rustDir, "component-scrap.jsonl");
var miscEvidencePath = Path.Combine(rustDir, "misc-scrap-breakdowns.jsonl");
var globalEvidencePath = Path.Combine(rustDir, "global-values.jsonl");
var formIdSlotsPath = Path.Combine(rustDir, "formid-slots.jsonl");
var conditionFunctionSchemaPath = Path.Combine(rustDir, "condition-function-schema.jsonl");
VerifyEvidenceHash(componentEvidencePath, rustCompleteRoot, "component_scrap_jsonl_sha256");
VerifyEvidenceHash(miscEvidencePath, rustCompleteRoot, "misc_scrap_breakdowns_jsonl_sha256");
VerifyEvidenceHash(globalEvidencePath, rustCompleteRoot, "global_values_jsonl_sha256");
VerifyEvidenceHash(formIdSlotsPath, rustCompleteRoot, "formid_slots_jsonl_sha256");
VerifyEvidenceHash(conditionFunctionSchemaPath, rustCompleteRoot, "condition_function_schema_jsonl_sha256");

using var censusDoc = JsonDocument.Parse(censusBytes);
var plugins = censusDoc.RootElement.GetProperty("plugins").EnumerateArray()
    .Select(row => row.GetProperty("name").GetString()!)
    .ToArray();
var expectedFiles = censusDoc.RootElement.GetProperty("files").EnumerateArray()
    .ToDictionary(row => row.GetProperty("path").GetString()!, StringComparer.OrdinalIgnoreCase);
var evidenceByPlugin = plugins.ToDictionary(name => name, _ => new List<RecipeDto>(), StringComparer.OrdinalIgnoreCase);
var options = new JsonSerializerOptions { PropertyNameCaseInsensitive = true };
var rustComponentScrap = ReadEvidenceLines<ScrapComponentEnvelope>(
    componentEvidencePath,
    censusHash,
    options);
var rustMiscBreakdowns = ReadEvidenceLines<MiscScrapEnvelope>(
    miscEvidencePath,
    censusHash,
    options);
var rustGlobalValues = ReadEvidenceLines<GlobalValueEnvelope>(
    globalEvidencePath,
    censusHash,
    options);
var rustFormIdSlots = ReadEvidenceLines<FormIdSlotEnvelope>(formIdSlotsPath, censusHash, options);
var rustConditionFunctionSchemas = ReadEvidenceLines<ConditionFunctionSchemaEnvelope>(
    conditionFunctionSchemaPath,
    censusHash,
    options);
foreach (var line in File.ReadLines(recipesPath, Encoding.UTF8))
{
    if (string.IsNullOrWhiteSpace(line)) continue;
    var envelope = JsonSerializer.Deserialize<EvidenceEnvelope>(line, options)
        ?? throw new InvalidDataException("empty Rust recipe row");
    if (envelope.SourceCensusSha256 != censusHash)
        throw new InvalidDataException("recipe row is linked to a different census");
    if (!evidenceByPlugin.TryGetValue(envelope.Recipe.Plugin, out var rows))
        throw new InvalidDataException("recipe row names a plugin outside the frozen census");
    rows.Add(envelope.Recipe);
}

Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);
var sourceStable = true;
long mutagenCobjRecords = 0;
var mismatches = new List<object>();
var pluginResults = new List<object>();
var rustConditionRows = new List<ConditionDto>();
var referenceRecipesAll = new List<ReferenceRecipe>();
var physicalItemRecords = new List<PhysicalItemRecord>();
var componentFormLinks = new List<ComponentFormLink>();
var physicalComponentRecords = new List<PhysicalComponentRecord>();
var miscBreakdownRecords = new List<MiscBreakdownRecord>();
var physicalGlobalRecords = new List<PhysicalGlobalRecord>();
var physicalSchemaFormLinks = new List<PhysicalSchemaFormLinks>();
foreach (var plugin in plugins)
{
    var relative = $"Data/{plugin}";
    if (!expectedFiles.TryGetValue(relative, out var fingerprint))
        throw new InvalidDataException($"missing frozen fingerprint for {plugin}");
    var expectedSha = fingerprint.GetProperty("sha256").GetString()!;
    var expectedBytes = fingerprint.GetProperty("bytes").GetUInt64();
    var path = Path.Combine(install, "Data", plugin);
    if (Path.GetFileName(path) != plugin || !File.Exists(path) || Directory.Exists(path)
        || File.GetAttributes(path).HasFlag(FileAttributes.ReparsePoint))
        throw new InvalidDataException($"unsafe or missing direct Data plugin {plugin}");
    var before = HashFile(path);
    var length = (ulong)new FileInfo(path).Length;
    if (before != expectedSha || length != expectedBytes)
        throw new InvalidDataException($"{plugin} differs from the frozen retail proof");

    using var mod = Fallout4Mod.CreateFromBinaryOverlay(
        new ModPath(ModKey.FromFileName(plugin), path), Fallout4Release.Fallout4);
    var referenceRecipes = mod.ConstructibleObjects.Records.Select(ToReferenceRecipe).ToArray();
    mutagenCobjRecords += referenceRecipes.Length;
    var sourceRecipes = evidenceByPlugin[plugin];
    rustConditionRows.AddRange(sourceRecipes.SelectMany(recipe => recipe.DecodedConditions));
    referenceRecipesAll.AddRange(referenceRecipes);
    physicalItemRecords.AddRange(EnumerateItems(mod).Select(item => new PhysicalItemRecord(
        plugin,
        item.RecordType,
        item.Record.FormKey.ToString(),
        item.Record.EditorID)));
    physicalComponentRecords.AddRange(mod.Components.Records.Select(record => new PhysicalComponentRecord(
        plugin,
        record.FormKey.ToString(),
        record.EditorID,
        record.AutoCalcValue,
        record.CraftingSound.FormKeyNullable?.ToString() ?? "Null",
        record.ScrapItem.FormKey.ToString(),
        record.ModScrapScalar.FormKey.ToString())));
    physicalSchemaFormLinks.AddRange(mod.Components.Records.Select(record => new PhysicalSchemaFormLinks(
        plugin,
        "CMPO",
        record.FormKey.ID,
        record.EditorID,
        new Dictionary<string, IReadOnlyList<string>>(StringComparer.Ordinal)
        {
            ["CraftingSound"] = [record.CraftingSound.FormKeyNullable?.ToString() ?? "Null"],
            ["ScrapItem"] = [record.ScrapItem.FormKey.ToString()],
            ["ModScrapScalar"] = [record.ModScrapScalar.FormKey.ToString()]
        })));
    physicalSchemaFormLinks.AddRange(mod.ConstructibleObjects.Records.Select(record =>
    {
        var functionConditions = record.Conditions
            .Select(condition => condition.Data)
            .OfType<IFunctionConditionDataGetter>()
            .ToArray();
        var firstFormConditions = functionConditions
            .Where(data => Condition.GetParameterTypes(data.Function).First.GetCategory() == Condition.ParameterCategory.Form)
            .Select(data => data.ParameterOneRecord.FormKey.ToString()).ToArray();
        var secondFormConditions = functionConditions
            .Where(data => Condition.GetParameterTypes(data.Function).Second.GetCategory() == Condition.ParameterCategory.Form)
            .Select(data => data.ParameterTwoRecord.FormKey.ToString()).ToArray();
        return new PhysicalSchemaFormLinks(
            plugin,
            "COBJ",
            record.FormKey.ID,
            record.EditorID,
            new Dictionary<string, IReadOnlyList<string>>(StringComparer.Ordinal)
            {
                ["CreatedObject"] = [record.CreatedObject.FormKeyNullable?.ToString() ?? "Null"],
                ["WorkbenchKeyword"] = [record.WorkbenchKeyword.FormKeyNullable?.ToString() ?? "Null"],
                ["MenuArtObject"] = [record.MenuArtObject.FormKeyNullable?.ToString() ?? "Null"],
                ["PickUpSound"] = [record.PickUpSound.FormKeyNullable?.ToString() ?? "Null"],
                ["PutDownSound"] = [record.PutDownSound.FormKeyNullable?.ToString() ?? "Null"],
                ["Component"] = record.Components?.Select(component => component.Component.FormKey.ToString()).ToArray() ?? [],
                ["Category"] = record.Categories?.Select(category => category.FormKey.ToString()).ToArray() ?? [],
                ["Condition.Reference"] = record.Conditions.Select(condition => condition.Data.Reference.FormKey.ToString()).ToArray(),
                ["Condition.ComparisonValue"] = record.Conditions.OfType<IConditionGlobalGetter>()
                    .Select(condition => condition.ComparisonValue.FormKey.ToString()).ToArray(),
                ["Condition.ParameterOneRecord"] = firstFormConditions,
                ["Condition.ParameterTwoRecord"] = secondFormConditions
            });
    }));
    miscBreakdownRecords.AddRange(mod.MiscItems.Records.Select(record => new MiscBreakdownRecord(
        plugin,
        record.FormKey.ToString(),
        record.EditorID,
        (record.Components ?? []).Select((component, index) => new MiscBreakdownLink(
            index,
            component.Component.FormKey.ToString(),
            component.Count)).ToArray(),
        record.ComponentDisplayIndices?.ToArray() ?? [])));
    physicalSchemaFormLinks.AddRange(mod.MiscItems.Records.Select(record => new PhysicalSchemaFormLinks(
        plugin,
        "MISC",
        record.FormKey.ID,
        record.EditorID,
        new Dictionary<string, IReadOnlyList<string>>(StringComparer.Ordinal)
        {
            ["PreviewTransform"] = [record.PreviewTransform.FormKeyNullable?.ToString() ?? "Null"],
            ["PickUpSound"] = [record.PickUpSound.FormKeyNullable?.ToString() ?? "Null"],
            ["PutDownSound"] = [record.PutDownSound.FormKeyNullable?.ToString() ?? "Null"],
            ["Keyword"] = record.Keywords?.Select(keyword => keyword.FormKey.ToString()).ToArray() ?? [],
            ["FeaturedItemMessage"] = [record.FeaturedItemMessage.FormKeyNullable?.ToString() ?? "Null"],
            ["Component"] = record.Components?.Select(component => component.Component.FormKey.ToString()).ToArray() ?? []
        })));
    physicalGlobalRecords.AddRange(mod.Globals.Records.Select(record =>
    {
        var global = (IGlobalGetter)record;
        return new PhysicalGlobalRecord(
            plugin,
            record.FormKey.ToString(),
            record.EditorID,
            global.TypeChar.ToString(),
            HasGlobalData(global));
    }));
    componentFormLinks.AddRange(mod.ConstructibleObjects.Records.SelectMany(recipe =>
        (recipe.Components ?? []).Select((component, index) => new ComponentFormLink(
            plugin,
            recipe.FormKey.ToString(),
            recipe.EditorID,
            index,
            component.Component.FormKey.ToString(),
            component.Component.FormKey.ID,
            component.Count))));
    var rustHistogram = Histogram(sourceRecipes.Select(RustSignature));
    var referenceHistogram = Histogram(referenceRecipes.Select(ReferenceSignature));
    var pluginMatches = rustHistogram.Count == referenceHistogram.Count
        && rustHistogram.All(pair => referenceHistogram.TryGetValue(pair.Key, out var count) && count == pair.Value);
    if (!pluginMatches)
    {
        foreach (var key in rustHistogram.Keys.Union(referenceHistogram.Keys).Order(StringComparer.Ordinal))
        {
            rustHistogram.TryGetValue(key, out var rustCount);
            referenceHistogram.TryGetValue(key, out var refCount);
            if (rustCount != refCount)
                mismatches.Add(new { plugin, signature = key, rust_count = rustCount, mutagen_count = refCount });
        }
    }
    var after = HashFile(path);
    if (after != before) sourceStable = false;
    pluginResults.Add(new
    {
        plugin,
        sha256 = before,
        source_bytes = length,
        rust_cobj_records = sourceRecipes.Count,
        mutagen_cobj_records = referenceRecipes.Length,
        rust_unique_structural_signatures = rustHistogram.Count,
        mutagen_unique_structural_signatures = referenceHistogram.Count,
        structural_histograms_match = pluginMatches,
        source_hash_stable = after == before
    });
}

var parameterSchemaAudit = AuditConditionParameterSchemas(rustConditionRows, referenceRecipesAll);
var functionSchemaTableAudit = AuditConditionFunctionSchemaTable(rustConditionFunctionSchemas);
var formLinkComparison = CompareSchemaFormLinks(
    rustFormIdSlots,
    physicalSchemaFormLinks,
    censusHash);
Directory.CreateDirectory(outputDir);
var ingredientCandidates = BuildIngredientCandidates(
    componentFormLinks,
    physicalItemRecords,
    evidenceByPlugin.Values.SelectMany(rows => rows).ToArray());
var componentConversion = BuildComponentConversionEvidence(
    physicalComponentRecords,
    miscBreakdownRecords,
    physicalItemRecords,
    physicalGlobalRecords);
var scalarGlobalRawEvidence = BuildScrapScalarRawEvidence(
    rustComponentScrap,
    rustGlobalValues,
    physicalComponentRecords,
    physicalGlobalRecords);
var scrapComparison = CompareScrapEvidence(
    rustComponentScrap,
    rustMiscBreakdowns,
    physicalComponentRecords,
    miscBreakdownRecords,
    censusHash);
mismatches.AddRange(scrapComparison.Mismatches);
var globalComparison = CompareGlobalValueEvidence(rustGlobalValues, physicalGlobalRecords, censusHash);
mismatches.AddRange(globalComparison.Mismatches);
mismatches.AddRange(formLinkComparison.Mismatches);
mismatches.AddRange(functionSchemaTableAudit.Mismatches);
var match = mismatches.Count == 0 && sourceStable && parameterSchemaAudit.Matches
    && functionSchemaTableAudit.Matches && scrapComparison.Matches
    && globalComparison.Matches && formLinkComparison.Matches;
var componentScrapPath = Path.Combine(outputDir, "component-scrap-links.jsonl");
var miscBreakdownPath = Path.Combine(outputDir, "misc-component-breakdowns.jsonl");
var scalarRawPath = Path.Combine(outputDir, "scrap-scalar-raw-values.jsonl");
await File.WriteAllLinesAsync(componentScrapPath,
    componentConversion.ComponentRows.Select(row => JsonSerializer.Serialize(row)), new UTF8Encoding(false));
await File.WriteAllLinesAsync(miscBreakdownPath,
    componentConversion.MiscRows.Select(row => JsonSerializer.Serialize(row)), new UTF8Encoding(false));
await File.WriteAllLinesAsync(scalarRawPath,
    scalarGlobalRawEvidence.Rows.Select(row => JsonSerializer.Serialize(row)), new UTF8Encoding(false));
var componentScrapHash = HashFile(componentScrapPath);
var miscBreakdownHash = HashFile(miscBreakdownPath);
var scalarRawHash = HashFile(scalarRawPath);
var ingredientCandidatesPath = Path.Combine(outputDir, "ingredient-candidates.jsonl");
await File.WriteAllLinesAsync(
    ingredientCandidatesPath,
    ingredientCandidates.Rows.Select(row => JsonSerializer.Serialize(row)),
    new UTF8Encoding(false));
var ingredientCandidatesHash = HashFile(ingredientCandidatesPath);
var comparisonPath = Path.Combine(outputDir, "comparison.json");
var comparison = new
{
    schema = 1,
    status = match ? "matched" : "mismatch",
    source_census_sha256 = censusHash,
    source_proof_complete_sha256 = sourceProofHash,
    rust_audit_complete_sha256 = Convert.ToHexString(SHA256.HashData(rustCompleteBytes)).ToLowerInvariant(),
    rust_recipes_sha256 = rustCompleteRoot.GetProperty("recipes_jsonl_sha256").GetString(),
    rust_global_values_sha256 = rustCompleteRoot.GetProperty("global_values_jsonl_sha256").GetString(),
    mutagen_revision = "4f533562ee0c70347d47c1979d5464d42b06ee6b",
    editor_id_join_fixtures_passed = true,
    compared_plugins = plugins.Length,
    rust_cobj_records = evidenceByPlugin.Values.Sum(rows => rows.Count),
    mutagen_cobj_records = mutagenCobjRecords,
    structural_histograms_match = mismatches.Count == 0,
    condition_parameter_schema_match = parameterSchemaAudit.Matches,
    condition_parameter_schema = parameterSchemaAudit,
    condition_function_schema_table_match = functionSchemaTableAudit.Matches,
    condition_function_schema_table = functionSchemaTableAudit.Summary,
    condition_function_schema_mismatches = functionSchemaTableAudit.Mismatches,
    raw_scrap_record_structure_match = scrapComparison.Matches,
    raw_scrap_record_structure = scrapComparison.Summary,
    raw_global_field_structure_match = globalComparison.Matches,
    raw_global_field_structure = globalComparison.Summary,
    raw_schema_form_links_match = formLinkComparison.Matches,
    raw_schema_form_links = formLinkComparison.Summary,
    scrap_scalar_raw_value_candidates = scalarGlobalRawEvidence.Summary,
    ingredient_identity_candidates = ingredientCandidates.Summary,
    component_conversion_structure = componentConversion.Summary,
    sources_hash_stable_before_and_after = sourceStable,
    comparison_dimensions = new[]
    {
        "per-record version",
        "component entry count and UInt32 quantity sequence",
        "category keyword link count",
        "raw CTDA condition count and fixed condition fields (flags, operator, comparison encoding, function index, run-on value)",
        "observed CTDA function names and first/second slot types/categories from the pinned FO4 parameter map",
        "all 479 pinned Fallout 4 condition-function names and three-slot parameter type/category rows",
        "exact same-plugin EDID/component-index pairing between Rust raw FVPA rows and Mutagen component FormKeys",
        "same-plugin strict EDID pairing for raw CMPO DATA/MNAM/GNAM and MISC CVPA/CDIX fields",
        "same-plugin low-24-bit numeric observations for CMPO/MISC FormID words, without assigning an ID conversion rule",
        "same-plugin strict EDID pairing for raw GLOB FNAM/FLTV presence, type byte and bounded value widths",
        "GLOB raw FLTV numeric bits retained but not compared to interpreted values",
        "all physical records implementing the pinned Fallout 4 IItem interface as identity candidates",
        "created-object link presence",
        "workbench keyword link presence",
        "INTV count, optional-priority presence and UInt16 priority"
    },
    limitations = new[]
    {
        "This is an independent structural comparison, not gameplay validation.",
        "Rust raw FVPA words are shown beside Mutagen FormKeys through exact same-plugin EDID plus component-index pairing; the numeric comparison is observational and assigns no raw-ID conversion rule.",
        "Ingredient FormKeys are Mutagen interpretations in each physical plugin's master context and are matched only to physical IItem candidates; they are not runtime identity or override winners.",
        "A function with no explicit parameter-map entry remains unresolved in Rust even when Mutagen defaults its parsed categories to None.",
        "The complete condition function/type comparison checks schema metadata only; it does not evaluate a condition or implement its native behavior.",
        "Condition evaluation, perk/material inventory, placement and refunds remain unimplemented."
    },
    plugins = pluginResults,
    mismatches
};
await File.WriteAllTextAsync(comparisonPath, JsonSerializer.Serialize(comparison, new JsonSerializerOptions { WriteIndented = true }));
var comparisonBytes = await File.ReadAllBytesAsync(comparisonPath);
var completion = new
{
    schema = 1,
    status = match ? "completed-independent-workshop-structure-comparison" : "completed-with-mismatches",
    private_directory = outputDir,
    comparison_sha256 = Convert.ToHexString(SHA256.HashData(comparisonBytes)).ToLowerInvariant(),
    ingredient_candidates_jsonl_sha256 = ingredientCandidatesHash,
    component_scrap_links_jsonl_sha256 = componentScrapHash,
    misc_component_breakdowns_jsonl_sha256 = miscBreakdownHash,
    scrap_scalar_raw_values_jsonl_sha256 = scalarRawHash,
    editor_id_join_fixtures_passed = true,
    ingredient_component_references = ingredientCandidates.Rows.Count,
    physical_item_candidates = physicalItemRecords.Count,
    physical_component_records = componentConversion.ComponentRows.Count,
    physical_misc_records = componentConversion.MiscRows.Count,
    physical_global_records = physicalGlobalRecords.Count,
    raw_global_records_compared = rustGlobalValues.Count,
    condition_function_schema_table = functionSchemaTableAudit.Summary,
    condition_function_schema_table_mismatches = functionSchemaTableAudit.Mismatches,
    raw_scrap_record_structure = scrapComparison.Summary,
    raw_global_field_structure = globalComparison.Summary,
    scrap_scalar_raw_value_candidates = scalarGlobalRawEvidence.Summary,
    result_matches = match,
    sources_hash_stable_before_and_after = sourceStable
};
await File.WriteAllTextAsync(
    Path.Combine(outputDir, "complete.json"),
    JsonSerializer.Serialize(completion, new JsonSerializerOptions { WriteIndented = true }));
Console.WriteLine(JsonSerializer.Serialize(completion, new JsonSerializerOptions { WriteIndented = true }));
return match ? 0 : 2;

static string HashFile(string path)
{
    using var stream = File.OpenRead(path);
    return Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
}

static void WriteConditionFixture(string outputDirectory)
{
    var directory = Path.GetFullPath(outputDirectory);
    var localRoot = Path.GetFullPath("local") + Path.DirectorySeparatorChar;
    if (!directory.StartsWith(localRoot, StringComparison.OrdinalIgnoreCase)
        || Directory.Exists(directory)
        || File.Exists(directory))
        throw new InvalidDataException("condition-fixture output must be a new directory under local/");

    Directory.CreateDirectory(directory);
    var modKey = ModKey.FromNameAndExtension("ConditionWriterFixture.esp");
    var mod = new Fallout4Mod(modKey, Fallout4Release.Fallout4);
    var recipe = new ConstructibleObject(mod, "ConditionWriterFixture");
    var comparisonGlobal = new GlobalFloat(mod, "ConditionWriterFixtureGlobal") { Data = 3.5f };
    mod.Globals.Add(comparisonGlobal);
    var data = new FunctionConditionData
    {
        Function = Condition.Function.GetStageDone,
        Unknown2 = 0x5A7C,
        ParameterTwoNumber = 0x11223344,
        RunOnType = Condition.RunOnType.LinkedReference,
        Unknown3 = -123,
    };
    data.ParameterOneRecord.FormKey = recipe.FormKey;
    data.Reference.FormKey = recipe.FormKey;
    recipe.Conditions.Add(new ConditionFloat
    {
        CompareOperator = CompareOperator.GreaterThanOrEqualTo,
        Flags = Condition.Flag.OR | Condition.Flag.ParametersUseAliases,
        Unknown1 = new byte[] { 0xA1, 0xB2, 0xC3 },
        ComparisonValue = 1.25f,
        Data = data,
    });
    var globalData = new FunctionConditionData
    {
        Function = Condition.Function.GetStageDone,
        Unknown2 = 0xBEEF,
        ParameterTwoNumber = 9,
        RunOnType = Condition.RunOnType.EventData,
        Unknown3 = 4567,
    };
    globalData.ParameterOneRecord.FormKey = recipe.FormKey;
    globalData.Reference.FormKey = recipe.FormKey;
    var globalCondition = new ConditionGlobal
    {
        CompareOperator = CompareOperator.LessThanOrEqualTo,
        Flags = Condition.Flag.OR | Condition.Flag.UsePackData,
        Unknown1 = new byte[] { 0xD4, 0xE5, 0xF6 },
        Data = globalData,
    };
    globalCondition.ComparisonValue.FormKey = comparisonGlobal.FormKey;
    recipe.Conditions.Add(globalCondition);
    var stringData = new FunctionConditionData
    {
        Function = Condition.Function.GetVMScriptVariable,
        ParameterOneString = "FixtureScript",
        ParameterTwoString = "FixtureVariable",
    };
    recipe.Conditions.Add(new ConditionFloat
    {
        CompareOperator = CompareOperator.EqualTo,
        Flags = Condition.Flag.OR,
        Data = stringData,
    });
    mod.ConstructibleObjects.Add(recipe);

    var pluginPath = Path.Combine(directory, "ConditionWriterFixture.esp");
    var modPath = new ModPath(modKey, pluginPath);
    mod.BeginWrite
        .ToPath(modPath)
        .WithNoLoadOrder()
        .NoModKeySync()
        .Write();

    var roundTrip = Fallout4Mod.CreateFromBinary(modPath, Fallout4Release.Fallout4);
    var roundTripRecipe = roundTrip.ConstructibleObjects.Records.Single();
    if (roundTripRecipe.Conditions.Count != 3)
        throw new InvalidDataException("Mutagen-authored COBJ did not retain all three CTDA conditions");
    var roundTripCondition = roundTripRecipe.Conditions[0];
    var roundTripGlobalCondition = roundTripRecipe.Conditions[1];
    var roundTripStringCondition = roundTripRecipe.Conditions[2];
    if (roundTripCondition is not IConditionFloatGetter floatCondition
        || floatCondition.Data is not IFunctionConditionDataGetter functionData
        || roundTripGlobalCondition is not IConditionGlobalGetter globalConditionGetter
        || globalConditionGetter.Data is not IFunctionConditionDataGetter globalFunctionData
        || roundTripStringCondition is not IConditionFloatGetter stringCondition
        || stringCondition.Data is not IFunctionConditionDataGetter stringFunctionData
        || functionData.Function != Condition.Function.GetStageDone
        || functionData.ParameterOneRecord.FormKey != roundTripRecipe.FormKey
        || functionData.ParameterTwoNumber != 0x11223344
        || functionData.Unknown2 != 0x5A7C
        || functionData.RunOnType != Condition.RunOnType.LinkedReference
        || functionData.Reference.FormKey != roundTripRecipe.FormKey
        || functionData.Unknown3 != -123
        || BitConverter.SingleToInt32Bits(floatCondition.ComparisonValue) != BitConverter.SingleToInt32Bits(1.25f)
        || floatCondition.Flags != (Condition.Flag.OR | Condition.Flag.ParametersUseAliases)
        || floatCondition.CompareOperator != CompareOperator.GreaterThanOrEqualTo
        || globalConditionGetter.ComparisonValue.FormKey != roundTrip.Globals.Records.Single().FormKey
        || globalFunctionData.Function != Condition.Function.GetStageDone
        || globalFunctionData.ParameterOneRecord.FormKey != roundTripRecipe.FormKey
        || globalFunctionData.ParameterTwoNumber != 9
        || globalFunctionData.Unknown2 != 0xBEEF
        || globalFunctionData.RunOnType != Condition.RunOnType.EventData
        || globalFunctionData.Reference.FormKey != roundTripRecipe.FormKey
        || globalFunctionData.Unknown3 != 4567
        || globalConditionGetter.Flags != (Condition.Flag.OR | Condition.Flag.UsePackData)
        || globalConditionGetter.CompareOperator != CompareOperator.LessThanOrEqualTo
        || stringFunctionData.Function != Condition.Function.GetVMScriptVariable
        || stringFunctionData.ParameterOneString != "FixtureScript"
        || stringFunctionData.ParameterTwoString != "FixtureVariable")
        throw new InvalidDataException("Mutagen-authored CTDA failed independent writer-to-reader round trip");

    var pluginBytes = File.ReadAllBytes(pluginPath);
    int[] FindSubrecordOffsets(string kind) => Enumerable.Range(0, pluginBytes.Length - 5)
        .Where(offset => pluginBytes.AsSpan(offset, 4).SequenceEqual(System.Text.Encoding.ASCII.GetBytes(kind)))
        .ToArray();
    byte[] ReadSubrecordPayload(int offset)
    {
        var length = pluginBytes[offset + 4] | (pluginBytes[offset + 5] << 8);
        if (offset + 6 + length > pluginBytes.Length)
            throw new InvalidDataException($"Subrecord {System.Text.Encoding.ASCII.GetString(pluginBytes, offset, 4)} is truncated");
        return pluginBytes.AsSpan(offset + 6, length).ToArray();
    }

    var ctdaOffsets = FindSubrecordOffsets("CTDA")
        .Where(offset => pluginBytes[offset + 4] == 32 && pluginBytes[offset + 5] == 0)
        .ToArray();
    var cis1Offsets = FindSubrecordOffsets("CIS1");
    var cis2Offsets = FindSubrecordOffsets("CIS2");
    if (ctdaOffsets.Length != 3 || cis1Offsets.Length != 1 || cis2Offsets.Length != 1)
        throw new InvalidDataException($"Expected three 32-byte CTDA records and one each of CIS1/CIS2; found {ctdaOffsets.Length}/{cis1Offsets.Length}/{cis2Offsets.Length}");
    var conditionSubrecordOrder = ctdaOffsets.Select(offset => (offset, "CTDA"))
        .Concat(cis1Offsets.Select(offset => (offset, "CIS1")))
        .Concat(cis2Offsets.Select(offset => (offset, "CIS2")))
        .OrderBy(row => row.offset)
        .Select(row => row.Item2)
        .ToArray();
    if (!conditionSubrecordOrder.SequenceEqual(new[] { "CTDA", "CTDA", "CTDA", "CIS1", "CIS2" }))
        throw new InvalidDataException("Mutagen did not emit CIS1/CIS2 after the string-parameter CTDA in COBJ order");
    var floatPayload = pluginBytes.AsSpan(ctdaOffsets[0] + 6, 32).ToArray();
    var globalPayload = pluginBytes.AsSpan(ctdaOffsets[1] + 6, 32).ToArray();
    var stringPayload = pluginBytes.AsSpan(ctdaOffsets[2] + 6, 32).ToArray();
    var floatPayloadPath = Path.Combine(directory, "mutagen-ctda-v1.bin");
    var globalPayloadPath = Path.Combine(directory, "mutagen-global-ctda-v1.bin");
    var stringPayloadPath = Path.Combine(directory, "mutagen-vmscript-ctda-v1.bin");
    File.WriteAllBytes(floatPayloadPath, floatPayload);
    File.WriteAllBytes(globalPayloadPath, globalPayload);
    File.WriteAllBytes(stringPayloadPath, stringPayload);
    var cis1Payload = ReadSubrecordPayload(cis1Offsets[0]);
    var cis2Payload = ReadSubrecordPayload(cis2Offsets[0]);
    var cis1PayloadPath = Path.Combine(directory, "mutagen-cis1-v1.bin");
    var cis2PayloadPath = Path.Combine(directory, "mutagen-cis2-v1.bin");
    File.WriteAllBytes(cis1PayloadPath, cis1Payload);
    File.WriteAllBytes(cis2PayloadPath, cis2Payload);

    var receipt = new
    {
        schema = 1,
        producer = "pinned Mutagen Fallout 4 writer and reader",
        mutagen_revision = "4f533562ee0c70347d47c1979d5464d42b06ee6b",
        fixture_plugin = Path.GetFileName(pluginPath),
        fixture_plugin_sha256 = HashFile(pluginPath),
        condition_count = 3,
        condition_subrecord_order = conditionSubrecordOrder,
        ctda_payloads = new[]
        {
            new
            {
                kind = "float-comparison",
                path = Path.GetFileName(floatPayloadPath),
                sha256 = HashFile(floatPayloadPath),
                bytes = floatPayload.Length,
                hex = Convert.ToHexString(floatPayload).ToLowerInvariant(),
            },
            new
            {
                kind = "global-formlink-comparison",
                path = Path.GetFileName(globalPayloadPath),
                sha256 = HashFile(globalPayloadPath),
                bytes = globalPayload.Length,
                hex = Convert.ToHexString(globalPayload).ToLowerInvariant(),
            },
            new
            {
                kind = "vmscript-string-parameters",
                path = Path.GetFileName(stringPayloadPath),
                sha256 = HashFile(stringPayloadPath),
                bytes = stringPayload.Length,
                hex = Convert.ToHexString(stringPayload).ToLowerInvariant(),
            },
        },
        string_companion_payloads = new[]
        {
            new { kind = "CIS1", path = Path.GetFileName(cis1PayloadPath), sha256 = HashFile(cis1PayloadPath), bytes = cis1Payload.Length, hex = Convert.ToHexString(cis1Payload).ToLowerInvariant() },
            new { kind = "CIS2", path = Path.GetFileName(cis2PayloadPath), sha256 = HashFile(cis2PayloadPath), bytes = cis2Payload.Length, hex = Convert.ToHexString(cis2Payload).ToLowerInvariant() },
        },
        independent_mutagen_round_trip = true,
        verified_fields = new[]
        {
            "comparison operator and float value",
            "global-comparison flag and Global FormLink",
            "GetVMScriptVariable string parameters written as separate CIS1/CIS2 companions",
            "condition flags and three unknown bytes",
            "GetStageDone function and schema-typed record/number parameters",
            "function unknown word",
            "run-on value and reference FormLink",
            "trailing unknown signed word",
        },
        self_links_are_fixture_local = true,
        runtime_formid_identity_assigned = false,
        condition_evaluation_performed = false,
    };
    File.WriteAllText(Path.Combine(directory, "receipt.json"), JsonSerializer.Serialize(receipt, new JsonSerializerOptions { WriteIndented = true }));
}

static bool HasGlobalData(IGlobalGetter record) => record switch
{
    IGlobalFloatGetter global => global.Data.HasValue,
    IGlobalIntGetter global => global.Data.HasValue,
    IGlobalShortGetter global => global.Data.HasValue,
    IGlobalBoolGetter global => global.Data.HasValue,
    _ => false
};

static void VerifyEvidenceHash(string path, JsonElement completion, string property)
{
    var expected = completion.GetProperty(property).GetString();
    if (expected is null || HashFile(path) != expected)
        throw new InvalidDataException($"Rust evidence file hash differs from completion marker: {property}");
}

static List<T> ReadEvidenceLines<T>(string path, string censusHash, JsonSerializerOptions options)
    where T : EvidenceLine
{
    var rows = new List<T>();
    foreach (var line in File.ReadLines(path, Encoding.UTF8))
    {
        if (string.IsNullOrWhiteSpace(line)) continue;
        var row = JsonSerializer.Deserialize<T>(line, options)
            ?? throw new InvalidDataException("empty Rust evidence row");
        if (row.SourceCensusSha256 != censusHash)
            throw new InvalidDataException("scrap evidence row is linked to a different census");
        rows.Add(row);
    }
    return rows;
}

static ScrapRecordComparison CompareScrapEvidence(
    IReadOnlyCollection<ScrapComponentEnvelope> rustComponents,
    IReadOnlyCollection<MiscScrapEnvelope> rustMisc,
    IReadOnlyCollection<PhysicalComponentRecord> referenceComponents,
    IReadOnlyCollection<MiscBreakdownRecord> referenceMisc,
    string censusHash)
{
    var mismatches = new List<object>();
    var componentMap = referenceComponents
        .GroupBy(row => EditorIDJoinKey(row.Plugin, row.EditorID), StringComparer.Ordinal)
        .ToDictionary(group => group.Key, group => group.ToArray(), StringComparer.Ordinal);
    var miscMap = referenceMisc
        .GroupBy(row => EditorIDJoinKey(row.Plugin, row.EditorID), StringComparer.Ordinal)
        .ToDictionary(group => group.Key, group => group.ToArray(), StringComparer.Ordinal);
    var seenComponentKeys = new HashSet<string>(StringComparer.Ordinal);
    var seenMiscKeys = new HashSet<string>(StringComparer.Ordinal);
    var componentJoinCount = 0;
    var componentHeaderIdsMatch = 0;
    var componentAutoCalcMatches = 0;
    var scrapItemLinksCompared = 0;
    var scrapItemLinksMatch = 0;
    var scalarLinksCompared = 0;
    var scalarLinksMatch = 0;
    var miscJoinCount = 0;
    var miscHeaderIdsMatch = 0;
    var miscDisplayIndexListsMatch = 0;
    var miscComponentEntriesCompared = 0;
    var miscComponentCountsMatch = 0;
    var miscComponentLocalIdsMatch = 0;

    foreach (var envelope in rustComponents)
    {
        var row = envelope.Component;
        var editorID = TryReadAsciiEditorIDFields(row.EditorIDs);
        if (editorID is null)
        {
            mismatches.Add(new { kind = "CMPO", plugin = row.Plugin, reason = "missing or malformed ASCII EDID" });
            continue;
        }
        var key = EditorIDJoinKey(row.Plugin, editorID);
        if (!componentMap.TryGetValue(key, out var candidates) || candidates.Length != 1)
        {
            mismatches.Add(new { kind = "CMPO", plugin = row.Plugin, editorID, candidate_count = candidates?.Length ?? 0 });
            continue;
        }
        if (!seenComponentKeys.Add(key))
        {
            mismatches.Add(new { kind = "CMPO", plugin = row.Plugin, editorID, reason = "duplicate Rust source EDID" });
            continue;
        }
        var reference = candidates[0];
        componentJoinCount++;
        var localId = TryReadLocalFormId(reference.MutagenFormKey);
        var headerIdMatches = localId is not null && (row.FormIdRaw & 0x00FF_FFFF) == localId.Value;
        if (headerIdMatches) componentHeaderIdsMatch++;
        var autoCalcMatches = reference.AutoCalcValue is uint value
            ? row.AutoCalcValues.SequenceEqual([value])
            : row.AutoCalcValues.Count == 0;
        if (autoCalcMatches) componentAutoCalcMatches++;
        var scrapItemMatches = RawLinkMatches(row.ScrapItems, reference.ScrapItemFormKey);
        var scalarMatches = RawLinkMatches(row.ScrapScalars, reference.ModScrapScalarFormKey);
        if (reference.ScrapItemFormKey != "Null")
        {
            scrapItemLinksCompared++;
            if (scrapItemMatches) scrapItemLinksMatch++;
        }
        if (reference.ModScrapScalarFormKey != "Null")
        {
            scalarLinksCompared++;
            if (scalarMatches) scalarLinksMatch++;
        }
        if (!headerIdMatches || !autoCalcMatches || !scrapItemMatches || !scalarMatches)
            mismatches.Add(new
            {
                kind = "CMPO",
                plugin = row.Plugin,
                editorID,
                header_local_id_match = headerIdMatches,
                auto_calc_value_sequence_match = autoCalcMatches,
                scrap_item_local_id_match = scrapItemMatches,
                scalar_global_local_id_match = scalarMatches
            });
    }

    foreach (var envelope in rustMisc)
    {
        var row = envelope.Misc;
        var editorID = TryReadAsciiEditorIDFields(row.EditorIDs);
        if (editorID is null)
        {
            mismatches.Add(new { kind = "MISC", plugin = row.Plugin, reason = "missing or malformed ASCII EDID" });
            continue;
        }
        var key = EditorIDJoinKey(row.Plugin, editorID);
        if (!miscMap.TryGetValue(key, out var candidates) || candidates.Length != 1)
        {
            mismatches.Add(new { kind = "MISC", plugin = row.Plugin, editorID, candidate_count = candidates?.Length ?? 0 });
            continue;
        }
        if (!seenMiscKeys.Add(key))
        {
            mismatches.Add(new { kind = "MISC", plugin = row.Plugin, editorID, reason = "duplicate Rust source EDID" });
            continue;
        }
        var reference = candidates[0];
        miscJoinCount++;
        var localId = TryReadLocalFormId(reference.MutagenFormKey);
        var headerIdMatches = localId is not null && (row.FormIdRaw & 0x00FF_FFFF) == localId.Value;
        if (headerIdMatches) miscHeaderIdsMatch++;
        var displaysMatch = row.ComponentDisplayIndices.Select(item => item.Value)
            .SequenceEqual(reference.ComponentDisplayIndices);
        if (displaysMatch) miscDisplayIndexListsMatch++;
        var sameLinkCount = row.Components.Count == reference.Components.Count;
        if (sameLinkCount)
        {
            for (var index = 0; index < row.Components.Count; index++)
            {
                var rustLink = row.Components[index];
                var referenceLink = reference.Components[index];
                var rawIdMatches = RawLinkMatches([rustLink.Component], referenceLink.MutagenFormKey);
                var countMatches = rustLink.Count == referenceLink.Count;
                miscComponentEntriesCompared++;
                if (rawIdMatches) miscComponentLocalIdsMatch++;
                if (countMatches) miscComponentCountsMatch++;
                if (!rawIdMatches || !countMatches)
                    mismatches.Add(new
                    {
                        kind = "MISC-CVPA",
                        plugin = row.Plugin,
                        editorID,
                        index,
                        local_id_match = rawIdMatches,
                        quantity_match = countMatches
                    });
            }
        }
        if (!headerIdMatches || !displaysMatch || !sameLinkCount)
            mismatches.Add(new
            {
                kind = "MISC",
                plugin = row.Plugin,
                editorID,
                header_local_id_match = headerIdMatches,
                component_display_indices_match = displaysMatch,
                component_entry_count_match = sameLinkCount
            });
    }

    if (componentJoinCount != referenceComponents.Count)
        mismatches.Add(new { kind = "CMPO", reason = "physical source rows were not joined one-to-one", rust_joined = componentJoinCount, reference = referenceComponents.Count });
    if (miscJoinCount != referenceMisc.Count)
        mismatches.Add(new { kind = "MISC", reason = "physical source rows were not joined one-to-one", rust_joined = miscJoinCount, reference = referenceMisc.Count });

    var matches = mismatches.Count == 0;
    var summary = new
    {
        scope = "strict same-plugin ASCII EDID joins; raw record/link low-24-bit numbers are compared with Mutagen local-ID candidates only",
        source_census_sha256 = censusHash,
        rust_component_records = rustComponents.Count,
        mutagen_component_records = referenceComponents.Count,
        component_records_joined = componentJoinCount,
        component_header_local_ids_match = componentHeaderIdsMatch,
        component_auto_calc_sequences_match = componentAutoCalcMatches,
        component_scrap_item_links_compared = scrapItemLinksCompared,
        component_scrap_item_links_match = scrapItemLinksMatch,
        component_scalar_links_compared = scalarLinksCompared,
        component_scalar_links_match = scalarLinksMatch,
        rust_misc_records = rustMisc.Count,
        mutagen_misc_records = referenceMisc.Count,
        misc_records_joined = miscJoinCount,
        misc_header_local_ids_match = miscHeaderIdsMatch,
        misc_display_index_lists_match = miscDisplayIndexListsMatch,
        misc_component_entries_compared = miscComponentEntriesCompared,
        misc_component_local_ids_match = miscComponentLocalIdsMatch,
        misc_component_quantities_match = miscComponentCountsMatch,
        runtime_identity_assigned = false,
        conversion_behavior_claimed = false,
        matches
    };
    return new ScrapRecordComparison(summary, mismatches, matches);
}

static uint? TryReadLocalFormId(string formKey)
{
    if (formKey == "Null") return null;
    var colon = formKey.IndexOf(':');
    if (colon <= 0 || colon > 8 || formKey.IndexOf(':', colon + 1) >= 0)
        return null;
    if (!uint.TryParse(formKey.AsSpan(0, colon), System.Globalization.NumberStyles.AllowHexSpecifier,
        System.Globalization.CultureInfo.InvariantCulture, out var localId)
        || localId > 0x00FF_FFFF)
        return null;
    return localId;
}

static GlobalFieldComparison CompareGlobalValueEvidence(
    IReadOnlyCollection<GlobalValueEnvelope> rustGlobals,
    IReadOnlyCollection<PhysicalGlobalRecord> referenceGlobals,
    string censusHash)
{
    var mismatches = new List<object>();
    var referenceByEditorID = referenceGlobals
        .GroupBy(row => EditorIDJoinKey(row.Plugin, row.EditorID), StringComparer.Ordinal)
        .ToDictionary(group => group.Key, group => group.ToArray(), StringComparer.Ordinal);
    var seen = new HashSet<string>(StringComparer.Ordinal);
    var joined = 0;
    var headerLocalIdsMatched = 0;
    var explicitTypeCharsCompared = 0;
    var explicitTypeCharsMatched = 0;
    var omittedDefaultFloatTypeChars = 0;
    var valuePresenceMatched = 0;
    var typedValueWidthsMatched = 0;
    var typedValueWidthsCompared = 0;

    foreach (var envelope in rustGlobals)
    {
        var row = envelope.Global;
        var editorID = TryReadAsciiEditorIDFields(row.EditorIDs);
        if (editorID is null)
        {
            mismatches.Add(new { kind = "GLOB", plugin = row.Plugin, reason = "missing or malformed ASCII EDID" });
            continue;
        }
        var key = EditorIDJoinKey(row.Plugin, editorID);
        if (!referenceByEditorID.TryGetValue(key, out var candidates) || candidates.Length != 1)
        {
            mismatches.Add(new { kind = "GLOB", plugin = row.Plugin, editorID, candidate_count = candidates?.Length ?? 0 });
            continue;
        }
        if (!seen.Add(key))
        {
            mismatches.Add(new { kind = "GLOB", plugin = row.Plugin, editorID, reason = "duplicate Rust source EDID" });
            continue;
        }
        var reference = candidates[0];
        joined++;
        var localId = TryReadLocalFormId(reference.MutagenFormKey);
        var headerIdMatches = localId is not null && (row.FormIdRaw & 0x00FF_FFFF) == localId.Value;
        if (headerIdMatches) headerLocalIdsMatched++;

        bool typeCharMatches;
        if (row.TypeCharSubrecords.Count == 0)
        {
            // The pinned schema omits FNAM for its default float type; Rust preserves the omission.
            typeCharMatches = reference.TypeChar == "f";
            if (typeCharMatches) omittedDefaultFloatTypeChars++;
        }
        else if (row.TypeCharSubrecords.Count == 1)
        {
            explicitTypeCharsCompared++;
            var rawBytes = TryHexBytes(row.TypeCharSubrecords[0].BytesHex);
            typeCharMatches = rawBytes is { Length: 1 }
                && rawBytes[0] <= 0x7F
                && reference.TypeChar.Length == 1
                && rawBytes[0] == (byte)reference.TypeChar[0];
            if (typeCharMatches) explicitTypeCharsMatched++;
        }
        else
        {
            typeCharMatches = false;
        }

        var valuePresenceMatches = row.ValueSubrecords.Count == (reference.HasData ? 1 : 0);
        if (valuePresenceMatches) valuePresenceMatched++;
        var typedType = reference.TypeChar is "f" or "l" or "s" or "b";
        var valueWidthsMatch = true;
        if (typedType)
        {
            typedValueWidthsCompared += row.ValueSubrecords.Count;
            valueWidthsMatch = row.ValueSubrecords.All(value => value.BytesHex.Length == 8);
            if (valueWidthsMatch) typedValueWidthsMatched += row.ValueSubrecords.Count;
        }

        if (!headerIdMatches || !typeCharMatches || !valuePresenceMatches || !valueWidthsMatch)
            mismatches.Add(new
            {
                kind = "GLOB",
                plugin = row.Plugin,
                editorID,
                header_local_id_match = headerIdMatches,
                type_char_match = typeCharMatches,
                value_presence_match = valuePresenceMatches,
                typed_value_widths_match = valueWidthsMatch
            });
    }
    if (joined != referenceGlobals.Count)
        mismatches.Add(new { kind = "GLOB", reason = "physical source rows were not joined one-to-one", rust_joined = joined, reference = referenceGlobals.Count });

    var matches = mismatches.Count == 0;
    var summary = new
    {
        scope = "strict same-plugin ASCII EDID joins; raw FNAM/FLTV evidence is retained and only pinned-schema type/presence/width fields are compared",
        source_census_sha256 = censusHash,
        rust_global_records = rustGlobals.Count,
        mutagen_global_records = referenceGlobals.Count,
        global_records_joined = joined,
        header_local_ids_matched = headerLocalIdsMatched,
        explicit_type_char_fields_compared = explicitTypeCharsCompared,
        explicit_type_char_fields_matched = explicitTypeCharsMatched,
        omitted_default_float_type_chars = omittedDefaultFloatTypeChars,
        value_presence_matches = valuePresenceMatched,
        typed_value_widths_compared = typedValueWidthsCompared,
        typed_value_widths_matched = typedValueWidthsMatched,
        raw_value_numeric_bits_compared = false,
        matches
    };
    return new GlobalFieldComparison(summary, mismatches, matches);
}

static byte[]? TryHexBytes(string text)
{
    try
    {
        return Convert.FromHexString(text);
    }
    catch (FormatException)
    {
        return null;
    }
}

static bool RawLinkMatches(IReadOnlyCollection<ScrapRawIdDto> rawLinks, string formKey)
{
    var localId = TryReadLocalFormId(formKey);
    if (localId is null)
        return rawLinks.Count == 0 || (rawLinks.Count == 1 && rawLinks.First().Raw == 0);
    return rawLinks.Count == 1 && (rawLinks.First().Raw & 0x00FF_FFFF) == localId.Value;
}

static SchemaFormLinkComparison CompareSchemaFormLinks(
    IReadOnlyCollection<FormIdSlotEnvelope> rustSlots,
    IReadOnlyCollection<PhysicalSchemaFormLinks> referenceRecords,
    string censusHash)
{
    var mismatches = new List<object>();
    var relevantRecords = rustSlots.Where(row => row.RecordKind is "COBJ" or "CMPO" or "MISC").ToArray();
    var relevantSlots = relevantRecords.Where(row => row.Field != "MajorRecord.FormID").ToArray();
    var rustByRecord = relevantRecords
        .GroupBy(row => SchemaRecordKey(row.Plugin, row.RecordKind, row.RecordFormIdRaw & 0x00FF_FFFF))
        .ToDictionary(group => group.Key, group => group.ToArray(), StringComparer.OrdinalIgnoreCase);
    var seenRecords = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
    var joinedRecords = 0;
    var comparedSlots = 0;
    var matchedSlots = 0;

    foreach (var reference in referenceRecords)
    {
        var key = SchemaRecordKey(reference.Plugin, reference.RecordKind, reference.LocalId);
        if (!rustByRecord.TryGetValue(key, out var actualRows))
        {
            mismatches.Add(new { kind = reference.RecordKind, plugin = reference.Plugin, local_id = reference.LocalId, reason = "Mutagen record has no raw slot evidence" });
            continue;
        }
        if (!seenRecords.Add(key))
        {
            mismatches.Add(new { kind = reference.RecordKind, plugin = reference.Plugin, local_id = reference.LocalId, reason = "ambiguous Mutagen record identity within one physical plugin" });
            continue;
        }
        joinedRecords++;
        var actualByField = actualRows.Where(row => row.Field != "MajorRecord.FormID")
            .GroupBy(row => row.Field, StringComparer.Ordinal)
            .ToDictionary(group => group.Key, group => group.OrderBy(row => row.Occurrence).ToArray(), StringComparer.Ordinal);
        foreach (var (field, expectedKeys) in reference.Fields)
        {
            actualByField.TryGetValue(field, out var actualFieldRows);
            actualFieldRows ??= [];
            var actualIds = actualFieldRows.Where(row => row.FormId.Raw != 0).Select(row => row.FormId.Raw).ToArray();
            var expectedIds = expectedKeys.Select(TryReadLocalFormId).Where(value => value.HasValue)
                .Select(value => value!.Value).ToArray();
            comparedSlots += Math.Max(actualIds.Length, expectedIds.Length);
            var fieldMatches = actualIds.Length == expectedIds.Length
                && actualIds.Zip(expectedIds).All(pair => (pair.First & 0x00FF_FFFF) == pair.Second);
            if (fieldMatches) matchedSlots += actualIds.Length;
            else mismatches.Add(new
            {
                kind = reference.RecordKind,
                plugin = reference.Plugin,
                editor_id = reference.EditorID,
                local_id = reference.LocalId,
                field,
                rust_non_null_slots = actualIds,
                mutagen_local_id_candidates = expectedIds
            });
        }
        var expectedFieldNames = reference.Fields.Keys.ToHashSet(StringComparer.Ordinal);
        foreach (var unexpectedField in actualByField.Keys.Where(name => !expectedFieldNames.Contains(name)))
            mismatches.Add(new { kind = reference.RecordKind, plugin = reference.Plugin, local_id = reference.LocalId, field = unexpectedField, reason = "raw link field has no counterpart in the pinned schema projection" });
    }
    if (joinedRecords != referenceRecords.Count)
        mismatches.Add(new { reason = "raw rows did not join one-to-one with Mutagen CMPO/MISC records", joined = joinedRecords, reference = referenceRecords.Count });
    if (seenRecords.Count != rustByRecord.Count)
        mismatches.Add(new { reason = "some raw link records had no unique Mutagen record", raw_records = rustByRecord.Count, joined_raw_records = seenRecords.Count });

    var matches = mismatches.Count == 0;
    return new SchemaFormLinkComparison(
        new
        {
            scope = "COBJ/CMPO/MISC schema-declared FormLink fields, including only explicitly mapped CTDA parameter slots; strict same-plugin physical record local-ID joins; Mutagen FormKeys are reduced to local-ID candidates for comparison",
            source_census_sha256 = censusHash,
            rust_slots = relevantSlots.Length,
            rust_records = rustByRecord.Count,
            mutagen_records = referenceRecords.Count,
            joined_records = joinedRecords,
            non_null_slots_compared = comparedSlots,
            local_id_candidates_matched = matchedSlots,
            raw_form_ids_resolved = false,
            matches
        },
        mismatches,
        matches);
}

static string SchemaRecordKey(string plugin, string recordKind, uint localId)
    => $"{plugin.ToUpperInvariant()}|{recordKind}|{localId:X6}";

static IngredientCandidateEvidence BuildIngredientCandidates(
    IReadOnlyCollection<ComponentFormLink> links,
    IReadOnlyCollection<PhysicalItemRecord> itemRecords,
    IReadOnlyCollection<RecipeDto> rustRecipes)
{
    var rustRecipesByEditorID = new Dictionary<string, List<RecipeDto>>(StringComparer.Ordinal);
    foreach (var recipe in rustRecipes)
    {
        var editorID = TryReadAsciiEditorID(recipe);
        if (editorID is null) continue;
        var key = EditorIDJoinKey(recipe.Plugin, editorID);
        if (!rustRecipesByEditorID.TryGetValue(key, out var editorIDRecipes))
            rustRecipesByEditorID.Add(key, editorIDRecipes = []);
        editorIDRecipes.Add(recipe);
    }
    var index = itemRecords.GroupBy(record => record.MutagenFormKey, StringComparer.Ordinal)
        .ToDictionary(
            group => group.Key,
            group => group.OrderBy(record => record.Plugin, StringComparer.OrdinalIgnoreCase)
                .ThenBy(record => record.EditorID, StringComparer.Ordinal).ToArray(),
            StringComparer.Ordinal);
    var rows = links.Select(link =>
    {
        index.TryGetValue(link.MutagenFormKey, out var candidates);
        candidates ??= [];
        rustRecipesByEditorID.TryGetValue(EditorIDJoinKey(link.SourcePlugin, link.RecipeEditorID), out var rawRecipes);
        rawRecipes ??= [];
        var rawRecipe = rawRecipes.Count == 1 ? rawRecipes[0] : null;
        var rawComponent = rawRecipe is not null && link.ComponentIndex < rawRecipe.Components.Count
            ? rawRecipe.Components[link.ComponentIndex]
            : null;
        var rawJoinStatus = rawRecipes.Count switch
        {
            0 => "no-exact-source-edid-match",
            > 1 => "multiple-exact-source-edid-matches",
            _ when rawComponent is null => "component-index-out-of-range",
            _ => "exact-source-edid-and-component-index-match"
        };
        var status = candidates.Length switch
        {
            0 => "no-physical-item-candidate",
            1 => "one-physical-item-candidate",
            _ => "multiple-physical-item-candidates"
        };
        return new IngredientCandidateRow(
            link.SourcePlugin,
            link.RecipeFormKey,
            link.RecipeEditorID,
            link.ComponentIndex,
            link.MutagenFormKey,
            link.MutagenLocalIdCandidate,
            link.Quantity,
            rawJoinStatus,
            rawRecipe?.FormIdRaw,
            rawComponent?.Component.Raw,
            rawComponent?.Count,
            rawComponent is not null && rawComponent.Count == link.Quantity,
            status,
            candidates);
    }).ToArray();
    var statusCounts = rows.GroupBy(row => row.CandidateStatus, StringComparer.Ordinal)
        .OrderBy(group => group.Key, StringComparer.Ordinal)
        .ToDictionary(group => group.Key, group => group.Count(), StringComparer.Ordinal);
    var summary = new
    {
        scope = "Mutagen-derived per-plugin COBJ component FormKeys matched against all direct physical FO4 IItem records in the frozen Data proof; every source candidate is retained",
        source_plugins = links.Select(link => link.SourcePlugin).Distinct(StringComparer.OrdinalIgnoreCase).Count(),
        physical_item_records = itemRecords.Count,
        component_references = rows.Length,
        unique_component_form_keys = rows.Select(row => row.MutagenFormKey).Distinct(StringComparer.Ordinal).Count(),
        candidate_status_counts = statusCounts,
        physical_target_record_type_counts = rows.SelectMany(row => row.PhysicalCandidates)
            .GroupBy(candidate => candidate.RecordType, StringComparer.Ordinal)
            .OrderBy(group => group.Key, StringComparer.Ordinal)
            .ToDictionary(group => group.Key, group => group.Count(), StringComparer.Ordinal),
        raw_edid_component_pairs = rows.Count(row => row.RustJoinStatus == "exact-source-edid-and-component-index-match"),
        raw_edid_component_quantity_matches = rows.Count(row => row.RustJoinStatus == "exact-source-edid-and-component-index-match" && row.RustQuantityMatchesReference),
        raw_edid_component_join_status_counts = rows.GroupBy(row => row.RustJoinStatus, StringComparer.Ordinal)
            .OrderBy(group => group.Key, StringComparer.Ordinal)
            .ToDictionary(group => group.Key, group => group.Count(), StringComparer.Ordinal),
        raw_form_id_vs_mutagen_local_id = new
        {
            scope = "side-by-side numeric observation only; no selector/master/runtime rule is inferred",
            exact_numeric_matches = rows.Count(row => row.RustComponentFormIdRaw == row.MutagenLocalIdCandidate),
            high_byte_only_differences = rows.Count(row => row.RustComponentFormIdRaw != row.MutagenLocalIdCandidate
                && (row.RustComponentFormIdRaw & 0x00FF_FFFF) == row.MutagenLocalIdCandidate),
            low_24_bit_differences = rows.Count(row => (row.RustComponentFormIdRaw & 0x00FF_FFFF) != row.MutagenLocalIdCandidate),
            raw_high_byte_counts = rows.GroupBy(row => (row.RustComponentFormIdRaw >> 24) & 0xFF)
                .OrderBy(group => group.Key)
                .ToDictionary(group => $"0x{group.Key:X2}", group => group.Count())
        },
        runtime_identity_assigned = false,
        override_winner_selected = false,
        raw_rust_form_ids_resolved = false
    };
    return new IngredientCandidateEvidence(rows, summary);
}

static string EditorIDJoinKey(string plugin, string? editorID) =>
    $"{plugin.ToUpperInvariant()}\0{editorID ?? "<null>"}";

static string? TryReadAsciiEditorID(RecipeDto recipe) => TryReadAsciiEditorIDFields(recipe.EditorIDs);

static string? TryReadAsciiEditorIDFields(IReadOnlyCollection<OpaqueDto> editorIDs)
{
    var rows = editorIDs.Where(row => row.Kind == "EDID").ToArray();
    if (rows.Length != 1) return null;
    byte[] bytes;
    try
    {
        bytes = Convert.FromHexString(rows[0].BytesHex);
    }
    catch (FormatException)
    {
        return null;
    }
    if (bytes.Length == 0 || bytes[^1] != 0 || bytes[..^1].Contains((byte)0)
        || bytes.Any(value => value > 0x7F))
        return null;
    return Encoding.ASCII.GetString(bytes, 0, bytes.Length - 1);
}

static void VerifyEditorIDJoinFixtures()
{
    static RecipeDto FromHex(params string[] values) => new()
    {
        EditorIDs = values.Select(value => new OpaqueDto { Kind = "EDID", BytesHex = value }).ToList()
    };

    if (TryReadAsciiEditorID(FromHex("464F345F5465737400")) != "FO4_Test")
        throw new InvalidDataException("valid ASCII EDID microfixture did not decode");
    var malformed = new[]
    {
        FromHex(),
        FromHex("4100", "4200"),
        FromHex("4142"),
        FromHex("41004200"),
        FromHex("418000"),
        FromHex("4Z00"),
        FromHex("410")
    };
    if (malformed.Any(recipe => TryReadAsciiEditorID(recipe) is not null))
        throw new InvalidDataException("malformed or ambiguous EDID microfixture was accepted");
}

static IEnumerable<(string RecordType, IItemGetter Record)> EnumerateItems(IFallout4ModGetter mod)
{
    foreach (var record in mod.Ammunitions.Records) yield return ("AMMO", record);
    foreach (var record in mod.Armors.Records) yield return ("ARMO", record);
    foreach (var record in mod.Books.Records) yield return ("BOOK", record);
    foreach (var record in mod.Components.Records) yield return ("CMPO", record);
    foreach (var record in mod.ConstructibleObjects.Records) yield return ("COBJ", record);
    foreach (var record in mod.Holotapes.Records) yield return ("NOTE", record);
    foreach (var record in mod.Ingestibles.Records) yield return ("ALCH", record);
    foreach (var record in mod.Ingredients.Records) yield return ("INGR", record);
    foreach (var record in mod.Keys.Records) yield return ("KEYM", record);
    foreach (var record in mod.LeveledItems.Records) yield return ("LVLI", record);
    foreach (var record in mod.Lights.Records) yield return ("LIGH", record);
    foreach (var record in mod.MiscItems.Records) yield return ("MISC", record);
    foreach (var record in mod.Weapons.Records) yield return ("WEAP", record);
}

static ComponentConversionEvidence BuildComponentConversionEvidence(
    IReadOnlyCollection<PhysicalComponentRecord> components,
    IReadOnlyCollection<MiscBreakdownRecord> miscRecords,
    IReadOnlyCollection<PhysicalItemRecord> items,
    IReadOnlyCollection<PhysicalGlobalRecord> globals)
{
    var miscByKey = items.Where(item => item.RecordType == "MISC")
        .GroupBy(item => item.MutagenFormKey, StringComparer.Ordinal)
        .ToDictionary(group => group.Key, group => group.OrderBy(item => item.Plugin, StringComparer.OrdinalIgnoreCase).ToArray(), StringComparer.Ordinal);
    var componentByKey = components.GroupBy(component => component.MutagenFormKey, StringComparer.Ordinal)
        .ToDictionary(group => group.Key, group => group.OrderBy(component => component.Plugin, StringComparer.OrdinalIgnoreCase).ToArray(), StringComparer.Ordinal);
    var globalByKey = globals.GroupBy(global => global.MutagenFormKey, StringComparer.Ordinal)
        .ToDictionary(group => group.Key, group => group.OrderBy(global => global.Plugin, StringComparer.OrdinalIgnoreCase).ToArray(), StringComparer.Ordinal);
    var componentRows = components.Select(component =>
    {
        miscByKey.TryGetValue(component.ScrapItemFormKey, out var scrapCandidates);
        scrapCandidates ??= [];
        globalByKey.TryGetValue(component.ModScrapScalarFormKey, out var scalarCandidates);
        scalarCandidates ??= [];
        return (object)new
        {
            component.Plugin,
            component.MutagenFormKey,
            component.EditorID,
            component.AutoCalcValue,
            component.ScrapItemFormKey,
            scrap_item_candidate_count = scrapCandidates.Length,
            scrap_item_candidates = scrapCandidates,
            component.ModScrapScalarFormKey,
            mod_scrap_scalar_candidate_count = scalarCandidates.Length,
            mod_scrap_scalar_candidates = scalarCandidates
        };
    }).ToArray();
    var miscRows = miscRecords.Select(record => (object)new
    {
        record.Plugin,
        record.MutagenFormKey,
        record.EditorID,
        component_links = record.Components.Select(link =>
        {
            componentByKey.TryGetValue(link.MutagenFormKey, out var candidates);
            candidates ??= [];
            return new
            {
                link.ComponentIndex,
                link.MutagenFormKey,
                link.Count,
                physical_component_candidate_count = candidates.Length,
                physical_component_candidates = candidates
            };
        }).ToArray(),
        record.ComponentDisplayIndices
    }).ToArray();
    var summary = new
    {
        scope = "pinned-schema CMPO ScrapItem/ModScrapScalar links and MISC CVPA component/count fields; physical candidates only",
        physical_component_records = components.Count,
        component_records_with_scrap_item_link = components.Count(component => component.ScrapItemFormKey != "Null"),
        component_records_with_one_physical_scrap_item_candidate = components.Count(component =>
            miscByKey.TryGetValue(component.ScrapItemFormKey, out var candidates) && candidates.Length == 1),
        physical_global_records = globals.Count,
        component_records_with_one_physical_scalar_global_candidate = components.Count(component =>
            globalByKey.TryGetValue(component.ModScrapScalarFormKey, out var candidates) && candidates.Length == 1),
        physical_misc_records = miscRecords.Count,
        misc_component_links = miscRecords.Sum(record => record.Components.Count),
        misc_component_links_with_one_physical_component_candidate = miscRecords.Sum(record => record.Components.Count(link =>
            componentByKey.TryGetValue(link.MutagenFormKey, out var candidates) && candidates.Length == 1)),
        runtime_scrap_behavior_claimed = false,
        scalar_global_values_read = false,
        inventory_conversion_or_scrap_quantity_evaluated = false
    };
    return new ComponentConversionEvidence(componentRows, miscRows, summary);
}

static (IReadOnlyList<object> Rows, object Summary) BuildScrapScalarRawEvidence(
    IReadOnlyCollection<ScrapComponentEnvelope> rustComponents,
    IReadOnlyCollection<GlobalValueEnvelope> rustGlobals,
    IReadOnlyCollection<PhysicalComponentRecord> components,
    IReadOnlyCollection<PhysicalGlobalRecord> globals)
{
    var rustComponentByEditorID = rustComponents.ToDictionary(
        row => EditorIDJoinKey(row.Component.Plugin, TryReadAsciiEditorIDFields(row.Component.EditorIDs)),
        row => row,
        StringComparer.Ordinal);
    var rustGlobalByEditorID = rustGlobals.ToDictionary(
        row => EditorIDJoinKey(row.Global.Plugin, TryReadAsciiEditorIDFields(row.Global.EditorIDs)),
        row => row,
        StringComparer.Ordinal);
    var globalsByKey = globals.GroupBy(row => row.MutagenFormKey, StringComparer.Ordinal)
        .ToDictionary(group => group.Key, group => group.ToArray(), StringComparer.Ordinal);
    var rows = new List<object>(components.Count);
    var noScalarLinkCount = 0;
    var uniquePhysicalCandidateCount = 0;
    var rawPayloadAssociatedCount = 0;
    var targetKeys = new HashSet<string>(StringComparer.Ordinal);
    var typeChars = new Dictionary<string, int>(StringComparer.Ordinal);

    foreach (var component in components)
    {
        var sourceKey = EditorIDJoinKey(component.Plugin, component.EditorID);
        rustComponentByEditorID.TryGetValue(sourceKey, out var rustSource);
        if (component.ModScrapScalarFormKey == "Null")
        {
            noScalarLinkCount++;
            rows.Add(new
            {
                component.Plugin,
                component.MutagenFormKey,
                component.EditorID,
                scalar_link_status = "null-link",
                raw_gnam_words = rustSource?.Component.ScrapScalars.Select(link => link.Raw).ToArray() ?? [],
                physical_global_candidates = Array.Empty<object>()
            });
            continue;
        }

        globalsByKey.TryGetValue(component.ModScrapScalarFormKey, out var candidates);
        candidates ??= [];
        if (candidates.Length == 1) uniquePhysicalCandidateCount++;
        var candidateRows = candidates.Select(candidate =>
        {
            targetKeys.Add(candidate.MutagenFormKey);
            typeChars[candidate.TypeChar] = typeChars.GetValueOrDefault(candidate.TypeChar) + 1;
            var globalKey = EditorIDJoinKey(candidate.Plugin, candidate.EditorID);
            rustGlobalByEditorID.TryGetValue(globalKey, out var rustGlobal);
            if (rustGlobal is not null) rawPayloadAssociatedCount++;
            var rawRecord = rustGlobal?.Global;
            return (object)new
            {
                candidate.Plugin,
                candidate.MutagenFormKey,
                candidate.EditorID,
                candidate.TypeChar,
                candidate.HasData,
                raw_type_char_subrecords = rawRecord?.TypeCharSubrecords.Select(field => new
                {
                    field.BytesHex,
                    field.Sha256
                }).ToArray() ?? [],
                raw_fltv_payloads = rawRecord?.ValueSubrecords.Select(field => new
                {
                    field.BytesHex,
                    field.Sha256,
                    field.PayloadOffset
                }).ToArray() ?? []
            };
        }).ToArray();
        rows.Add(new
        {
            component.Plugin,
            component.MutagenFormKey,
            component.EditorID,
            scalar_link_status = candidates.Length == 1
                ? "one-physical-global-candidate"
                : candidates.Length == 0 ? "no-physical-global-candidate" : "multiple-physical-global-candidates",
            scalar_global_form_key_candidate = component.ModScrapScalarFormKey,
            raw_gnam_words = rustSource?.Component.ScrapScalars.Select(link => link.Raw).ToArray() ?? [],
            physical_global_candidate_count = candidates.Length,
            physical_global_candidates = candidateRows
        });
    }

    var summary = new
    {
        scope = "raw GLOB FNAM/FLTV bytes attached to CMPO scalar-link physical candidates through pinned Mutagen FormKeys; no numeric conversion or behavior claim",
        component_records = components.Count,
        null_scalar_links = noScalarLinkCount,
        non_null_scalar_links = components.Count - noScalarLinkCount,
        links_with_one_physical_global_candidate = uniquePhysicalCandidateCount,
        candidate_records_with_raw_rust_global_row = rawPayloadAssociatedCount,
        unique_physical_global_form_key_candidates = targetKeys.Count,
        physical_candidate_type_char_counts = typeChars,
        fltv_numeric_bits_interpreted = false,
        runtime_identity_assigned = false
    };
    return (rows, summary);
}

static Dictionary<string, int> Histogram(IEnumerable<string> signatures)
{
    var result = new Dictionary<string, int>(StringComparer.Ordinal);
    foreach (var signature in signatures) result[signature] = result.GetValueOrDefault(signature) + 1;
    return result;
}

static string RustSignature(RecipeDto recipe)
{
    var created = recipe.CreatedObjects.Count != 0 && recipe.CreatedObjects.All(row => row.Raw != 0);
    var workbench = recipe.WorkbenchKeywords.Count != 0 && recipe.WorkbenchKeywords.All(row => row.Raw != 0);
    var conditionCount = recipe.ConditionSubrecords.Count(row => row.Kind == "CTDA");
    var shape = new
    {
        recipe.RecordVersion,
        ComponentCount = recipe.Components.Count,
        ComponentQuantities = recipe.Components.Select(row => row.Count).ToArray(),
        CategoryCount = recipe.Categories.Count,
        ConditionCount = conditionCount,
        Conditions = recipe.DecodedConditions.Select(ConditionShape.FromRust).ToArray(),
        HasCreatedObject = created,
        HasWorkbenchKeyword = workbench,
        CreatedCounts = recipe.CreatedObjectCounts.Select(row => new
        {
            row.Count,
            PriorityPresent = row.Priority.HasValue,
            Priority = row.Priority
        }).ToArray()
    };
    return JsonSerializer.Serialize(shape);
}

static string ReferenceSignature(ReferenceRecipe recipe)
{
    var shape = new
    {
        recipe.RecordVersion,
        ComponentCount = recipe.ComponentQuantities.Length,
        ComponentQuantities = recipe.ComponentQuantities,
        CategoryCount = recipe.CategoryCount,
        ConditionCount = recipe.ConditionCount,
        Conditions = recipe.Conditions,
        recipe.HasCreatedObject,
        recipe.HasWorkbenchKeyword,
        CreatedCounts = recipe.CreatedCounts.Select(row => new
        {
            row.Count,
            PriorityPresent = !row.PriorityMissing,
            Priority = row.PriorityMissing ? (ushort?)null : row.Priority
        }).ToArray()
    };
    return JsonSerializer.Serialize(shape);
}

static ReferenceRecipe ToReferenceRecipe(IConstructibleObjectGetter record)
{
    var countItems = record.CreatedObjectCounts ?? [];
    return new ReferenceRecipe(
        record.FormVersion,
        record.Components?.Select(component => component.Count).ToArray() ?? [],
        record.Categories?.Count ?? 0,
        record.Conditions.Count,
        record.CreatedObject.FormKey != FormKey.Null,
        record.WorkbenchKeyword.FormKey != FormKey.Null,
        record.Conditions.Select(ConditionShape.FromReference).ToArray(),
        record.Conditions.Select(ConditionParameterSchema.FromReference).ToArray(),
        countItems.Select(item => new ReferenceCount(
            item.Count,
            item.Priority,
            item.Versioning.HasFlag(ConstructibleCreatedObjectCount.VersioningBreaks.Break0))).ToArray());
}

static ConditionSchemaAudit AuditConditionParameterSchemas(
    IReadOnlyCollection<ConditionDto> rustConditions,
    IReadOnlyCollection<ReferenceRecipe> referenceRecipes)
{
    var rustGroups = rustConditions.GroupBy(condition => condition.FunctionIndex)
        .ToDictionary(group => group.Key, group => group.ToArray());
    var referenceGroups = referenceRecipes.SelectMany(recipe => recipe.ConditionSchemas)
        .GroupBy(condition => condition.FunctionIndex)
        .ToDictionary(group => group.Key, group => group.ToArray());
    var rows = new List<ConditionSchemaRow>();
    var matches = true;
    foreach (var functionIndex in rustGroups.Keys.Union(referenceGroups.Keys).Order())
    {
        rustGroups.TryGetValue(functionIndex, out var rustRows);
        referenceGroups.TryGetValue(functionIndex, out var referenceRows);
        rustRows ??= [];
        referenceRows ??= [];
        var rustHint = rustRows.FirstOrDefault()?.FunctionParameterHint;
        var referenceHint = referenceRows.FirstOrDefault();
        var countsMatch = rustRows.Length == referenceRows.Length;
        var rustHintsConsistent = rustRows.Select(row => JsonSerializer.Serialize(row.FunctionParameterHint))
            .Distinct(StringComparer.Ordinal).Count() <= 1;
        var referenceHintsConsistent = referenceRows.Select(row => JsonSerializer.Serialize(row))
            .Distinct(StringComparer.Ordinal).Count() <= 1;
        var hintMatches = (rustHint, referenceHint) switch
        {
            (null, null) => true,
            (null, not null) => false,
            (not null, null) => false,
            (not null, not null) when rustHint.MappingStatus == "explicit" =>
                rustHint.FunctionName == referenceHint.FunctionName
                && rustHint.ParameterOneType == referenceHint.ParameterOneType
                && rustHint.ParameterOneCategory == referenceHint.ParameterOneCategory.ToLowerInvariant()
                && rustHint.ParameterTwoType == referenceHint.ParameterTwoType
                && rustHint.ParameterTwoCategory == referenceHint.ParameterTwoCategory.ToLowerInvariant()
                && rustHint.ParameterThreeType == referenceHint.ParameterThreeType
                && rustHint.ParameterThreeCategory == referenceHint.ParameterThreeCategory.ToLowerInvariant(),
            (not null, not null) when rustHint.MappingStatus == "function-enum-known-parameter-map-defaulted" =>
                rustHint.FunctionName == referenceHint.FunctionName
                && rustHint.ParameterOneCategory == "unresolved"
                && rustHint.ParameterTwoCategory == "unresolved"
                && rustHint.ParameterThreeCategory == "unresolved"
                && referenceHint.ParameterOneType == "None"
                && referenceHint.ParameterTwoType == "None"
                && referenceHint.ParameterThreeType == "None",
            (not null, not null) => rustHint.FunctionName is null
                && rustHint.MappingStatus == "unknown-function-index"
                && referenceHint.FunctionName is null
                && referenceHint.ParameterOneType == "None"
                && referenceHint.ParameterTwoType == "None"
                && referenceHint.ParameterThreeType == "None",
        };
        var rowMatches = countsMatch && rustHintsConsistent && referenceHintsConsistent && hintMatches;
        matches &= rowMatches;
        rows.Add(new ConditionSchemaRow(
            functionIndex,
            rustRows.Length,
            rustHint?.FunctionName,
            rustHint?.MappingStatus,
            rustHint?.ParameterOneType,
            rustHint?.ParameterOneCategory,
            rustHint?.ParameterTwoType,
            rustHint?.ParameterTwoCategory,
            rustHint?.ParameterThreeType,
            rustHint?.ParameterThreeCategory,
            referenceHint?.FunctionName,
            referenceHint?.ParameterOneType,
            referenceHint?.ParameterOneCategory,
            referenceHint?.ParameterTwoType,
            referenceHint?.ParameterTwoCategory,
            referenceHint?.ParameterThreeType,
            referenceHint?.ParameterThreeCategory,
            countsMatch,
            rustHintsConsistent,
            referenceHintsConsistent,
            rowMatches));
    }
    return new ConditionSchemaAudit(rustConditions.Count, rows, matches);
}

static FunctionSchemaTableAudit AuditConditionFunctionSchemaTable(
    IReadOnlyCollection<ConditionFunctionSchemaEnvelope> rustRows)
{
    var expected = Enum.GetValues<Condition.Function>()
        .Distinct()
        .OrderBy(function => (ushort)function)
        .Select(function =>
        {
            var (first, second, third) = Condition.GetParameterTypes(function);
            var hasExplicitMap = first != Condition.ParameterType.None
                || second != Condition.ParameterType.None
                || third != Condition.ParameterType.None;
            return new
            {
                FunctionIndex = (ushort)function,
                FunctionName = Enum.GetName(function),
                MappingStatus = hasExplicitMap
                    ? "explicit"
                    : "function-enum-known-parameter-map-defaulted",
                ParameterOneType = first.ToString(),
                ParameterOneCategory = first.GetCategory().ToString().ToLowerInvariant(),
                ParameterTwoType = second.ToString(),
                ParameterTwoCategory = second.GetCategory().ToString().ToLowerInvariant(),
                ParameterThreeType = third.ToString(),
                ParameterThreeCategory = third.GetCategory().ToString().ToLowerInvariant()
            };
        })
        .ToArray();
    var expectedIds = expected.Select(row => row.FunctionIndex).ToHashSet();
    var rustGroups = rustRows.GroupBy(row => row.FunctionIndex)
        .ToDictionary(group => group.Key, group => group.ToArray());
    var mismatches = new List<object>();
    var namesMatched = 0;
    var mappingStatusesMatched = 0;
    var parameterTypesMatched = 0;
    var parameterCategoriesMatched = 0;
    var defaultedRowsRetainedUnresolved = 0;
    var joinedFunctions = 0;

    foreach (var reference in expected)
    {
        if (!rustGroups.TryGetValue(reference.FunctionIndex, out var matches))
        {
            mismatches.Add(new { function_index = reference.FunctionIndex, reason = "missing Rust schema row" });
            continue;
        }
        if (matches.Length != 1)
        {
            mismatches.Add(new
            {
                function_index = reference.FunctionIndex,
                reason = "duplicate Rust schema rows",
                count = matches.Length
            });
            continue;
        }

        joinedFunctions++;
        var rust = matches[0];
        var nameMatches = rust.FunctionName == reference.FunctionName;
        var statusMatches = rust.MappingStatus == reference.MappingStatus;
        var isExplicit = reference.MappingStatus == "explicit";
        var typesMatch = isExplicit
            ? rust.ParameterOneType == reference.ParameterOneType
                && rust.ParameterTwoType == reference.ParameterTwoType
                && rust.ParameterThreeType == reference.ParameterThreeType
            : rust.ParameterOneType == "Unspecified"
                && rust.ParameterTwoType == "Unspecified"
                && rust.ParameterThreeType == "Unspecified"
                && reference.ParameterOneType == "None"
                && reference.ParameterTwoType == "None"
                && reference.ParameterThreeType == "None";
        var categoriesMatch = isExplicit
            ? rust.ParameterOneCategory == reference.ParameterOneCategory
                && rust.ParameterTwoCategory == reference.ParameterTwoCategory
                && rust.ParameterThreeCategory == reference.ParameterThreeCategory
            : rust.ParameterOneCategory == "unresolved"
                && rust.ParameterTwoCategory == "unresolved"
                && rust.ParameterThreeCategory == "unresolved"
                && reference.ParameterOneCategory == "none"
                && reference.ParameterTwoCategory == "none"
                && reference.ParameterThreeCategory == "none";
        if (nameMatches) namesMatched++;
        if (statusMatches) mappingStatusesMatched++;
        if (isExplicit && typesMatch) parameterTypesMatched++;
        if (isExplicit && categoriesMatch) parameterCategoriesMatched++;
        if (!isExplicit && typesMatch && categoriesMatch) defaultedRowsRetainedUnresolved++;
        if (!nameMatches || !statusMatches || !typesMatch || !categoriesMatch)
        {
            mismatches.Add(new
            {
                function_index = reference.FunctionIndex,
                rust.FunctionName,
                mutagen_function_name = reference.FunctionName,
                rust.MappingStatus,
                mutagen_mapping_status = reference.MappingStatus,
                rust.ParameterOneType,
                rust.ParameterOneCategory,
                rust.ParameterTwoType,
                rust.ParameterTwoCategory,
                rust.ParameterThreeType,
                rust.ParameterThreeCategory,
                mutagen_parameter_one_type = reference.ParameterOneType,
                mutagen_parameter_one_category = reference.ParameterOneCategory,
                mutagen_parameter_two_type = reference.ParameterTwoType,
                mutagen_parameter_two_category = reference.ParameterTwoCategory,
                mutagen_parameter_three_type = reference.ParameterThreeType,
                mutagen_parameter_three_category = reference.ParameterThreeCategory
            });
        }
    }

    var unexpectedRows = rustRows.Count(row => !expectedIds.Contains(row.FunctionIndex));
    if (unexpectedRows != 0)
        mismatches.Add(new { reason = "unexpected Rust schema rows", count = unexpectedRows });
    var countsMatch = rustRows.Count == expected.Length && rustGroups.Count == expected.Length;
    var explicitExpected = expected.Count(row => row.MappingStatus == "explicit");
    var defaultedExpected = expected.Length - explicitExpected;
    if (!countsMatch)
        mismatches.Add(new { reason = "schema row count differs", rust_rows = rustRows.Count, mutagen_functions = expected.Length });
    var matchesAll = mismatches.Count == 0
        && namesMatched == expected.Length
        && mappingStatusesMatched == expected.Length
        && parameterTypesMatched == explicitExpected
        && parameterCategoriesMatched == explicitExpected
        && defaultedRowsRetainedUnresolved == defaultedExpected;
    var summary = new
    {
        mutagen_function_count = expected.Length,
        rust_schema_rows = rustRows.Count,
        joined_functions = joinedFunctions,
        names_matched = namesMatched,
        mapping_statuses_matched = mappingStatusesMatched,
        explicit_parameter_type_rows_matched = parameterTypesMatched,
        explicit_parameter_category_rows_matched = parameterCategoriesMatched,
        explicit_mapping_rows = rustRows.Count(row => row.MappingStatus == "explicit"),
        defaulted_named_rows = rustRows.Count(row => row.MappingStatus == "function-enum-known-parameter-map-defaulted"),
        defaulted_rows_retained_unresolved = defaultedRowsRetainedUnresolved,
        unknown_rows = rustRows.Count(row => row.MappingStatus == "unknown-function-index"),
        counts_match = countsMatch,
        all_rows_match = matchesAll
    };
    return new FunctionSchemaTableAudit(summary, mismatches, matchesAll);
}

sealed record ReferenceRecipe(
    ushort RecordVersion,
    uint[] ComponentQuantities,
    int CategoryCount,
    int ConditionCount,
    bool HasCreatedObject,
    bool HasWorkbenchKeyword,
    ConditionShape[] Conditions,
    ConditionParameterSchema[] ConditionSchemas,
    ReferenceCount[] CreatedCounts);

sealed record ReferenceCount(ushort Count, ushort Priority, bool PriorityMissing);

sealed record ConditionShape(
    ushort FunctionIndex,
    byte FlagBits,
    byte CompareOperatorBits,
    bool ComparisonValueIsGlobalFormId,
    uint RunOnRaw)
{
    public static ConditionShape FromRust(ConditionDto condition) => new(
        condition.FunctionIndex,
        condition.FlagBits,
        condition.CompareOperatorBits,
        condition.ComparisonValueIsGlobalFormId,
        condition.RunOnRaw);

    public static ConditionShape FromReference(IConditionGetter condition)
    {
        var global = condition is IConditionGlobalGetter;
        var function = condition.Data is IFunctionConditionDataGetter functionData
            ? (ushort)functionData.Function
            : (ushort)4672;
        return new ConditionShape(
            function,
            (byte)((byte)condition.Flags | (global ? 0x04 : 0)),
            (byte)condition.CompareOperator,
            global,
            (uint)condition.Data.RunOnType);
    }
}

sealed record ConditionParameterSchema(
    ushort FunctionIndex,
    string? FunctionName,
    string ParameterOneType,
    string ParameterOneCategory,
    string ParameterTwoType,
    string ParameterTwoCategory,
    string ParameterThreeType,
    string ParameterThreeCategory)
{
    public static ConditionParameterSchema FromReference(IConditionGetter condition)
    {
        var functionIndex = condition.Data is IFunctionConditionDataGetter functionData
            ? (ushort)functionData.Function
            : (ushort)4672;
        var functionName = Enum.GetName((Condition.Function)functionIndex);
        var (first, second, third) = Condition.GetParameterTypes((Condition.Function)functionIndex);
        return new ConditionParameterSchema(
            functionIndex,
            functionName,
            first.ToString(),
            first.GetCategory().ToString(),
            second.ToString(),
            second.GetCategory().ToString(),
            third.ToString(),
            third.GetCategory().ToString());
    }
}

sealed record ConditionSchemaRow(
    ushort FunctionIndex,
    int ConditionCount,
    string? RustFunctionName,
    string? RustMappingStatus,
    string? RustParameterOneType,
    string? RustParameterOneCategory,
    string? RustParameterTwoType,
    string? RustParameterTwoCategory,
    string? RustParameterThreeType,
    string? RustParameterThreeCategory,
    string? MutagenFunctionName,
    string? MutagenParameterOneType,
    string? MutagenParameterOneCategory,
    string? MutagenParameterTwoType,
    string? MutagenParameterTwoCategory,
    string? MutagenParameterThreeType,
    string? MutagenParameterThreeCategory,
    bool ConditionCountsMatch,
    bool RustHintsConsistent,
    bool MutagenHintsConsistent,
    bool Matches);

sealed record ConditionSchemaAudit(int Conditions, IReadOnlyList<ConditionSchemaRow> Functions, bool Matches);

sealed record PhysicalItemRecord(string Plugin, string RecordType, string MutagenFormKey, string? EditorID);

sealed record PhysicalComponentRecord(
    string Plugin,
    string MutagenFormKey,
    string? EditorID,
    uint? AutoCalcValue,
    string CraftingSoundFormKey,
    string ScrapItemFormKey,
    string ModScrapScalarFormKey);

sealed record PhysicalSchemaFormLinks(
    string Plugin,
    string RecordKind,
    uint LocalId,
    string? EditorID,
    IReadOnlyDictionary<string, IReadOnlyList<string>> Fields);

sealed record PhysicalGlobalRecord(string Plugin, string MutagenFormKey, string? EditorID, string TypeChar, bool HasData);

sealed record MiscBreakdownRecord(
    string Plugin,
    string MutagenFormKey,
    string? EditorID,
    IReadOnlyList<MiscBreakdownLink> Components,
    IReadOnlyList<byte> ComponentDisplayIndices);

sealed record MiscBreakdownLink(int ComponentIndex, string MutagenFormKey, uint Count);

sealed record ComponentConversionEvidence(
    IReadOnlyList<object> ComponentRows,
    IReadOnlyList<object> MiscRows,
    object Summary);

sealed record ComponentFormLink(
    string SourcePlugin,
    string RecipeFormKey,
    string? RecipeEditorID,
    int ComponentIndex,
    string MutagenFormKey,
    uint MutagenLocalIdCandidate,
    uint Quantity);

sealed record IngredientCandidateRow(
    string SourcePlugin,
    string RecipeFormKey,
    string? RecipeEditorID,
    int ComponentIndex,
    string MutagenFormKey,
    uint MutagenLocalIdCandidate,
    uint Quantity,
    string RustJoinStatus,
    uint? RustRecipeFormIdRaw,
    uint? RustComponentFormIdRaw,
    uint? RustComponentQuantity,
    bool RustQuantityMatchesReference,
    string CandidateStatus,
    IReadOnlyList<PhysicalItemRecord> PhysicalCandidates);

sealed record IngredientCandidateEvidence(IReadOnlyList<IngredientCandidateRow> Rows, object Summary);

sealed record ScrapRecordComparison(object Summary, IReadOnlyList<object> Mismatches, bool Matches);
sealed record GlobalFieldComparison(object Summary, IReadOnlyList<object> Mismatches, bool Matches);
sealed record SchemaFormLinkComparison(object Summary, IReadOnlyList<object> Mismatches, bool Matches);
sealed record FunctionSchemaTableAudit(object Summary, IReadOnlyList<object> Mismatches, bool Matches);

abstract class EvidenceLine
{
    [JsonPropertyName("source_census_sha256")] public string SourceCensusSha256 { get; set; } = "";
}

sealed class ScrapComponentEnvelope : EvidenceLine
{
    [JsonPropertyName("component")] public ScrapComponentDto Component { get; set; } = new();
}

sealed class MiscScrapEnvelope : EvidenceLine
{
    [JsonPropertyName("misc")] public MiscScrapDto Misc { get; set; } = new();
}

sealed class GlobalValueEnvelope : EvidenceLine
{
    [JsonPropertyName("global")] public GlobalValueDto Global { get; set; } = new();
}

sealed class FormIdSlotEnvelope : EvidenceLine
{
    [JsonPropertyName("plugin")] public string Plugin { get; set; } = "";
    [JsonPropertyName("record_kind")] public string RecordKind { get; set; } = "";
    [JsonPropertyName("record_form_id_raw")] public uint RecordFormIdRaw { get; set; }
    [JsonPropertyName("field")] public string Field { get; set; } = "";
    [JsonPropertyName("occurrence")] public int Occurrence { get; set; }
    [JsonPropertyName("form_id")] public FormIdObservationDto FormId { get; set; } = new();
}

sealed class ConditionFunctionSchemaEnvelope : EvidenceLine
{
    [JsonPropertyName("function_index")] public ushort FunctionIndex { get; set; }
    [JsonPropertyName("function_name")] public string? FunctionName { get; set; }
    [JsonPropertyName("mapping_status")] public string MappingStatus { get; set; } = "";
    [JsonPropertyName("parameter_one_type")] public string ParameterOneType { get; set; } = "";
    [JsonPropertyName("parameter_one_category")] public string ParameterOneCategory { get; set; } = "";
    [JsonPropertyName("parameter_two_type")] public string ParameterTwoType { get; set; } = "";
    [JsonPropertyName("parameter_two_category")] public string ParameterTwoCategory { get; set; } = "";
    [JsonPropertyName("parameter_three_type")] public string ParameterThreeType { get; set; } = "";
    [JsonPropertyName("parameter_three_category")] public string ParameterThreeCategory { get; set; } = "";
}

sealed class FormIdObservationDto
{
    [JsonPropertyName("raw")] public uint Raw { get; set; }
}

sealed class ScrapComponentDto
{
    [JsonPropertyName("plugin")] public string Plugin { get; set; } = "";
    [JsonPropertyName("form_id_raw")] public uint FormIdRaw { get; set; }
    [JsonPropertyName("editor_ids")] public List<OpaqueDto> EditorIDs { get; set; } = [];
    [JsonPropertyName("auto_calc_values")] public List<uint> AutoCalcValues { get; set; } = [];
    [JsonPropertyName("scrap_items")] public List<ScrapRawIdDto> ScrapItems { get; set; } = [];
    [JsonPropertyName("scrap_scalars")] public List<ScrapRawIdDto> ScrapScalars { get; set; } = [];
}

sealed class MiscScrapDto
{
    [JsonPropertyName("plugin")] public string Plugin { get; set; } = "";
    [JsonPropertyName("form_id_raw")] public uint FormIdRaw { get; set; }
    [JsonPropertyName("editor_ids")] public List<OpaqueDto> EditorIDs { get; set; } = [];
    [JsonPropertyName("components")] public List<MiscScrapComponentDto> Components { get; set; } = [];
    [JsonPropertyName("component_display_indices")] public List<ComponentDisplayIndexDto> ComponentDisplayIndices { get; set; } = [];
}

sealed class GlobalValueDto
{
    [JsonPropertyName("plugin")] public string Plugin { get; set; } = "";
    [JsonPropertyName("form_id_raw")] public uint FormIdRaw { get; set; }
    [JsonPropertyName("editor_ids")] public List<OpaqueDto> EditorIDs { get; set; } = [];
    [JsonPropertyName("type_char_subrecords")] public List<OpaqueDto> TypeCharSubrecords { get; set; } = [];
    [JsonPropertyName("value_subrecords")] public List<OpaqueDto> ValueSubrecords { get; set; } = [];
}

sealed class ScrapRawIdDto
{
    [JsonPropertyName("raw")] public uint Raw { get; set; }
}

sealed class MiscScrapComponentDto
{
    [JsonPropertyName("component")] public ScrapRawIdDto Component { get; set; } = new();
    [JsonPropertyName("count")] public uint Count { get; set; }
}

sealed class ComponentDisplayIndexDto
{
    [JsonPropertyName("value")] public byte Value { get; set; }
}

sealed class EvidenceEnvelope
{
    [JsonPropertyName("source_census_sha256")] public string SourceCensusSha256 { get; set; } = "";
    [JsonPropertyName("recipe")] public RecipeDto Recipe { get; set; } = new();
}

sealed class RecipeDto
{
    [JsonPropertyName("plugin")] public string Plugin { get; set; } = "";
    [JsonPropertyName("form_id_raw")] public uint FormIdRaw { get; set; }
    [JsonPropertyName("editor_ids")] public List<OpaqueDto> EditorIDs { get; set; } = [];
    [JsonPropertyName("record_version")] public ushort RecordVersion { get; set; }
    [JsonPropertyName("created_objects")] public List<RawIdDto> CreatedObjects { get; set; } = [];
    [JsonPropertyName("workbench_keywords")] public List<RawIdDto> WorkbenchKeywords { get; set; } = [];
    [JsonPropertyName("components")] public List<ComponentDto> Components { get; set; } = [];
    [JsonPropertyName("categories")] public List<RawIdDto> Categories { get; set; } = [];
    [JsonPropertyName("created_object_counts")] public List<CreatedCountDto> CreatedObjectCounts { get; set; } = [];
    [JsonPropertyName("condition_subrecords")] public List<OpaqueDto> ConditionSubrecords { get; set; } = [];
    [JsonPropertyName("decoded_conditions")] public List<ConditionDto> DecodedConditions { get; set; } = [];
}

sealed class ConditionDto
{
    [JsonPropertyName("function_index")] public ushort FunctionIndex { get; set; }
    [JsonPropertyName("flag_bits")] public byte FlagBits { get; set; }
    [JsonPropertyName("compare_operator_bits")] public byte CompareOperatorBits { get; set; }
    [JsonPropertyName("comparison_value_is_global_form_id")] public bool ComparisonValueIsGlobalFormId { get; set; }
    [JsonPropertyName("run_on_raw")] public uint RunOnRaw { get; set; }
    [JsonPropertyName("function_parameter_hint")] public FunctionParameterHintDto FunctionParameterHint { get; set; } = new();
}

sealed class FunctionParameterHintDto
{
    [JsonPropertyName("function_name")] public string? FunctionName { get; set; }
    [JsonPropertyName("mapping_status")] public string MappingStatus { get; set; } = "";
    [JsonPropertyName("parameter_one_type")] public string ParameterOneType { get; set; } = "";
    [JsonPropertyName("parameter_one_category")] public string ParameterOneCategory { get; set; } = "";
    [JsonPropertyName("parameter_two_type")] public string ParameterTwoType { get; set; } = "";
    [JsonPropertyName("parameter_two_category")] public string ParameterTwoCategory { get; set; } = "";
    [JsonPropertyName("parameter_three_type")] public string ParameterThreeType { get; set; } = "";
    [JsonPropertyName("parameter_three_category")] public string ParameterThreeCategory { get; set; } = "";
}

sealed class RawIdDto
{
    [JsonPropertyName("raw")] public uint Raw { get; set; }
}

sealed class ComponentDto
{
    [JsonPropertyName("component")] public RawIdDto Component { get; set; } = new();
    [JsonPropertyName("count")] public uint Count { get; set; }
}

sealed class CreatedCountDto
{
    [JsonPropertyName("count")] public ushort Count { get; set; }
    [JsonPropertyName("priority")] public ushort? Priority { get; set; }
}

sealed class OpaqueDto
{
    [JsonPropertyName("kind")] public string Kind { get; set; } = "";
    [JsonPropertyName("bytes_hex")] public string BytesHex { get; set; } = "";
    [JsonPropertyName("sha256")] public string Sha256 { get; set; } = "";
    [JsonPropertyName("payload_offset")] public int PayloadOffset { get; set; }
}
