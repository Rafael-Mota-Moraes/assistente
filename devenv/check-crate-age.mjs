// Verifica que todo crate do Cargo.lock foi publicado há pelo menos N dias (padrão 14):
// proteção contra versões comprometidas recém-publicadas. Usa as datas (`pubtime`) do
// índice esparso do crates.io.
//   node devenv/check-crate-age.mjs [dias]        (via `just idade-crates`)
import { readFileSync } from "node:fs";

const minDays = Number(process.argv[2] ?? 14);
const cutoff = new Date(Date.now() - minDays * 86_400_000);

const lock = readFileSync(new URL("../Cargo.lock", import.meta.url), "utf8");
const packages = [
  ...lock.matchAll(/\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"\nsource = "registry/g),
].map(([, name, version]) => ({ name, version }));

function indexPath(name) {
  const n = name.toLowerCase();
  if (n.length <= 2) return `${n.length}/${n}`;
  if (n.length === 3) return `3/${n[0]}/${n}`;
  return `${n.slice(0, 2)}/${n.slice(2, 4)}/${n}`;
}

async function publishTimes(name) {
  const response = await fetch(`https://index.crates.io/${indexPath(name)}`);
  if (!response.ok) throw new Error(`${name}: HTTP ${response.status}`);
  const text = await response.text();
  return new Map(
    text
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line))
      .map((entry) => [entry.vers, entry.pubtime]),
  );
}

const names = [...new Set(packages.map((p) => p.name))];
const times = new Map();
for (let i = 0; i < names.length; i += 32) {
  const batch = names.slice(i, i + 32);
  const results = await Promise.all(batch.map(publishTimes));
  batch.forEach((name, j) => times.set(name, results[j]));
}

const tooNew = packages
  .map(({ name, version }) => ({ name, version, pubtime: times.get(name).get(version) }))
  .filter(({ pubtime }) => !pubtime || new Date(pubtime) > cutoff);

if (tooNew.length > 0) {
  console.error(`Crates publicados há menos de ${minDays} dias (corte ${cutoff.toISOString()}):`);
  for (const { name, version, pubtime } of tooNew) {
    console.error(`  ${name} ${version} ${pubtime ?? "(sem data no índice)"}`);
  }
  process.exit(1);
}
console.log(`${packages.length} crates, todos publicados há pelo menos ${minDays} dias.`);
