// Runs the Omarchy plugin marketplace's own Automated Security Baseline against a commit of this repository,
// serving GitHub's API and raw files from the local Git object store instead of the network, so the result is the
// one the marketplace's bot would report for the same commit.
//
//   node tests/marketplace-baseline.mjs --marketplace=PATH_TO_MARKETPLACE_CHECKOUT [--commit=SHA]
//
// Prints the outcome, findings and capabilities, and exits 1 unless the result is the expected one: review
// required for the bundled renderer binary alone.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { pathToFileURL } from "node:url";

function option(name, fallback) {
  const prefix = `--${name}=`;
  const value = process.argv.find((argument) => argument.startsWith(prefix))?.slice(prefix.length);
  if (value === undefined && fallback === undefined) throw new Error(`${prefix}... is required`);
  return value ?? fallback;
}

const repository = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const marketplace = path.resolve(option("marketplace"));
const git = (...args) => execFileSync("git", ["-C", repository, ...args], { maxBuffer: 256 * 1024 * 1024 });
const commit = git("rev-parse", option("commit", "HEAD")).toString().trim();
const treeSha = git("rev-parse", `${commit}^{tree}`).toString().trim();
const [owner, name] = ["jeremydixon22", "omarchy-screensafe"];

const tree = git("ls-tree", "-r", "-t", "-l", "-z", commit)
  .toString()
  .split("\0")
  .filter(Boolean)
  .map((line) => {
    const [meta, file] = line.split("\t");
    const [mode, type, sha, size] = meta.split(/\s+/);
    return type === "tree" ? { path: file, mode, type, sha } : { path: file, mode, type, sha, size: Number(size) };
  });

function json(body) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
}

async function fetchLocal(url, options = {}) {
  const target = new URL(url);
  if (target.host === "api.github.com") {
    const base = `/repos/${owner}/${name}`;
    if (target.pathname === base) return json({ full_name: `${owner}/${name}`, private: false, disabled: false, archived: false, default_branch: "main" });
    if (target.pathname === `${base}/commits/${commit}`) return json({ sha: commit, commit: { tree: { sha: treeSha } } });
    if (target.pathname === `${base}/git/trees/${treeSha}`) return json({ sha: treeSha, tree, truncated: false });
    return new Response("{}", { status: 404 });
  }
  if (target.host === "raw.githubusercontent.com") {
    const prefix = `/${owner}/${name}/${commit}/`;
    if (!target.pathname.startsWith(prefix)) return new Response("", { status: 404 });
    const file = target.pathname.slice(prefix.length).split("/").map(decodeURIComponent).join("/");
    const bytes = git("show", `${commit}:${file}`);
    const range = new Headers(options.headers || {}).get("range");
    if (range) {
      const [, start, end] = range.match(/^bytes=(\d+)-(\d+)$/).map(Number);
      const last = Math.min(end, bytes.length - 1);
      return new Response(bytes.subarray(start, last + 1), {
        status: 206,
        headers: { "content-range": `bytes ${start}-${last}/${bytes.length}` },
      });
    }
    return new Response(bytes, { status: 200 });
  }
  return new Response("", { status: 404 });
}

const scanner = await import(pathToFileURL(path.join(marketplace, "scripts", "security-baseline-scanner.mjs")).href);
const manifest = JSON.parse(git("show", `${commit}:manifest.json`).toString());
const result = await scanner.runSecurityBaseline(`https://github.com/${owner}/${name}`, commit, {
  fetchImpl: fetchLocal,
  token: "",
  requiredPaths: Object.values(manifest.entryPoints),
  listedPlugins: [{ pluginId: manifest.id, manifestPathHint: "manifest.json" }],
});
const marketplaceCommit = execFileSync("git", ["-C", marketplace, "rev-parse", "HEAD"]).toString().trim();
const findings = result.findings.map((f) => f.ruleId).sort();
const capabilities = result.capabilities.map((c) => c.id).sort();
console.log(JSON.stringify({
  commit,
  marketplaceCommit,
  outcome: result.outcome,
  blocksApproval: result.blocksApproval,
  findings,
  capabilities,
  evidence: result.capabilities.flatMap((c) => c.evidence.map((e) => `${c.id}: ${e.path}:${e.line} ${e.snippet}`)),
}, null, 2));
const expected = result.outcome === "review-required" && !result.blocksApproval && findings.length === 0
  && JSON.stringify(capabilities) === JSON.stringify(["bundled-executable-binary"]);
process.exit(expected ? 0 : 1);
