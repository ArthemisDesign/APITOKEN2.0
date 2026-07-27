#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat, readdir, readlink, realpath } from "node:fs/promises";
import path from "node:path";

function fail(message) {
  process.stderr.write(`release-tree-digest: ${message}\n`);
  process.exit(1);
}

async function fileDigest(file) {
  const hash = createHash("sha256");
  await new Promise((resolve, reject) => {
    const stream = createReadStream(file);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("error", reject);
    stream.on("end", resolve);
  });
  return hash.digest("hex");
}

if (process.argv.length !== 3) {
  fail("usage: release-tree-digest.mjs <release-tree>");
}

const input = path.resolve(process.argv[2]);
let inputStat;
try {
  inputStat = await lstat(input);
} catch {
  fail(`release tree is missing: ${input}`);
}
if (!inputStat.isDirectory() || inputStat.isSymbolicLink()) {
  fail(`release tree must be a real directory: ${input}`);
}

const root = await realpath(input);
const aggregate = createHash("sha256");

function record(...fields) {
  aggregate.update(JSON.stringify(fields));
  aggregate.update("\n");
}

function isInsideRoot(candidate) {
  return candidate === root || candidate.startsWith(`${root}${path.sep}`);
}

async function walk(directory) {
  const entries = await readdir(directory);
  entries.sort((left, right) => Buffer.from(left).compare(Buffer.from(right)));

  for (const name of entries) {
    const absolute = path.join(directory, name);
    const relative = path.relative(root, absolute);
    const stat = await lstat(absolute);
    const mode = (stat.mode & 0o777).toString(8).padStart(3, "0");

    if (stat.isDirectory()) {
      record("directory", relative, mode);
      await walk(absolute);
      continue;
    }
    if (stat.isFile()) {
      record("file", relative, mode, stat.size, await fileDigest(absolute));
      continue;
    }
    if (stat.isSymbolicLink()) {
      const target = await readlink(absolute);
      if (path.isAbsolute(target)) {
        fail(`absolute symlink is not relocatable: ${relative} -> ${target}`);
      }
      const lexicalTarget = path.resolve(path.dirname(absolute), target);
      if (!isInsideRoot(lexicalTarget)) {
        fail(`symlink escapes release tree: ${relative} -> ${target}`);
      }
      record("symlink", relative, mode, target);
      continue;
    }
    fail(`special filesystem entry is forbidden: ${relative}`);
  }
}

await walk(root);
process.stdout.write(`${aggregate.digest("hex")}\n`);
