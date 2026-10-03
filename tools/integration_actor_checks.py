"""Fresh owner-authored placement and hostile body inputs, using frozen tools."""
import copy

from integration_proof import digest, document, require, write_json


def authored_actor_checks(root, run, cli, actor, binaries, revision, execute, powershell):
    fixtures = root / "local/integration-actor-fixtures"
    placement = fixtures / "act03-placement-authored-inputs-20261003"
    hostile = fixtures / "act08-body-prefix-negatives-20261003"
    require(document(placement / "negative-order.json") == document(hostile / "order.json"),
            "Malformed and hostile fixture load orders differ")
    directory = run / "actor-authored"
    execute("actor-authored-placements", [*powershell, root / "tools/actor-oracle/compare.ps1",
            "-Fallout", cli, "-Oracle", actor, "-Install", placement / "install",
            "-LoadOrder", placement / "order.json", "-RunDirectory", directory,
            "-IncludePlacements", "-AllowSourceFindings"])
    receipt = document(directory / "worker-comparison.json")
    require(receipt["started_source_revision"] == revision and receipt["source_and_binaries_unchanged"] is True,
            "Authored actor source identity differs")
    require(receipt["engine_binary_sha256"] == binaries[cli.relative_to(root).as_posix()]
            and receipt["oracle_binary_sha256"] == binaries[actor.relative_to(root).as_posix()],
            "Authored actor binary identity differs")
    require([phase["name"] for phase in receipt["phases"]] == ["cold", "warm", "reordered"],
            "Authored placement phases omitted")
    reports = {}
    for phase in receipt["phases"]:
        name = phase["name"]
        rust_path = directory / f"{name}.json"
        native_path = directory / ("reordered-oracle.json" if name == "reordered" else "oracle.json")
        require(digest(rust_path) == phase["rust_report_sha256"]
                and digest(native_path) == phase["oracle_report_sha256"], "Authored actor report differs")
        rust, native = document(rust_path), document(native_path)
        require(rust["independent_comparison"]["equal"] is True and phase["exit_code"] == 1,
                "Authored actor comparison/finding differs")
        for key in ("schema_version", "profile", "sources", "metadata", "winning_content_sha256",
                    "counts", "definitions", "actor_placements"):
            require(rust[key] == native[key], f"Authored {name} projection differs: {key}")
        reports[name] = rust
    cold, warm = copy.copy(reports["cold"]), copy.copy(reports["warm"])
    cold.pop("index_cache", None); warm.pop("index_cache", None)
    require(cold == warm, "Authored cold/warm placement source facts differ")
    require(reports["cold"]["actor_placements"] != reports["reordered"]["actor_placements"],
            "Authored independent override reorder did not change source winner facts")
    native = document(directory / "oracle.json")
    rows = native["actor_placements"]["definitions"]
    live = next(row for row in rows if row["core"] is not None)
    modifier = next(field["value"] for row in rows for field in row["fields"]
                    if field["value"]["kind"] == "level_modifier")
    alterations = []
    for name, target, field in (("core", live["core"]["position_bits"], 0),
                                ("parent", live["parent"], "cell"),
                                ("extra", modifier, "modifier")):
        original = target[field]
        target[field] = original ^ 1
        altered = directory / f"altered-{name}.json"
        write_json(altered, native)
        target[field] = original
        reason = "independent actor source comparison differs in actor_placements"
        execute(f"actor-authored-altered-{name}", [cli, "actor-sources", "--install", placement / "install",
                "--load-order", placement / "order.json", "--include-placements", "--compare-oracle", altered,
                "--output", directory / f"rejected-{name}.json"], expected=1, diagnostic=reason)
        alterations.append({"name": name, "diagnostic": reason})
    malformed = []
    for name, native_reason, rust_reason in (
        ("bad-level-modifier", "unsupported placed actor extra extent", "XLCM length 3"),
        ("bad-zone", "unsupported placed actor extra extent", "XEZN length 5"),
        ("missing-core", "incomplete placed actor fields/core", "placement lacks NAME"),
        ("nonfinite-transform", "nonfinite placed actor transform", "non-finite"),
        ("unsupported-version", "unsupported placed actor record version", "ACHR placed actor record version 14"),
    ):
        install = placement / name
        execute(f"actor-malformed-{name}-native", [actor, install / "Data", hostile / "order.bin",
                "--include-placements"], expected=1, diagnostic=native_reason)
        execute(f"actor-malformed-{name}-rust", [cli, "actor-sources", "--install", install,
                "--load-order", placement / "negative-order.json", "--include-placements",
                "--output", directory / f"malformed-{name}.json"], expected=1, diagnostic=rust_reason)
        malformed.append({"name": name, "native_diagnostic": native_reason, "rust_diagnostic": rust_reason})
    body = []
    for kind, mode in (("NPC_", []), ("CLAS", ["--include-classes"]),
                       ("FACT", ["--include-factions"]), ("ACHR", ["--include-placements"])):
        install = hostile / kind
        native_reason, rust_reason = "actor source declared body budget before inflate", "decompression budget exceeded"
        execute(f"actor-body-{kind}-native", [actor, install / "Data", hostile / "order.bin", *mode],
                expected=1, diagnostic=native_reason)
        execute(f"actor-body-{kind}-rust", [cli, "actor-sources", "--install", install,
                "--load-order", hostile / "order.json", *mode,
                "--output", directory / f"hostile-{kind}.json"], expected=1, diagnostic=rust_reason)
        body.append({"kind": kind, "native_diagnostic": native_reason, "rust_diagnostic": rust_reason})
    return {"fixture_manifest_sha256": digest(fixtures / "manifest.json"),
            "placement_phase_counts": {name: report["actor_placements"]["counts"] for name, report in reports.items()},
            "altered_projection_diagnostics": alterations, "malformed_source_diagnostics": malformed,
            "hostile_body_diagnostics": body, "gameplay_accepted": False}
