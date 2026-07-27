#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const [repoArgument, base, target] = process.argv.slice(2);
const shaPattern = /^[0-9a-f]{40}$/;

function fail(message) {
  throw new Error(message);
}

function git(repo, ...arguments_) {
  return execFileSync("git", ["-C", repo, ...arguments_], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function selectFull(reason) {
  process.stderr.write(`[typescript-scope] full workspace: ${reason}\n`);
  process.stdout.write("full\n");
  process.exit(0);
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function workspaceManifests(repo) {
  const manifests = [];
  for (const parent of ["apps", "packages"]) {
    for (const entry of readdirSync(resolve(repo, parent), { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const manifestPath = resolve(repo, parent, entry.name, "package.json");
      if (!existsSync(manifestPath)) continue;
      manifests.push({
        directory: `${parent}/${entry.name}`,
        path: manifestPath,
      });
    }
  }
  return manifests;
}

function changedEntries(repo) {
  const raw = execFileSync(
    "git",
    [
      "-C",
      repo,
      "diff",
      "--name-status",
      "-z",
      "--no-renames",
      "--diff-filter=ACDMRTUXB",
      `${base}..${target}`,
    ],
    { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
  );
  const fields = raw.split("\0");
  if (fields.at(-1) === "") fields.pop();
  if (fields.length % 2 !== 0) fail("git returned an incomplete changed-path record");

  const entries = [];
  for (let index = 0; index < fields.length; index += 2) {
    const status = fields[index];
    const path = fields[index + 1];
    if (!/^[ACDMRTUXB]$/.test(status) || !path) {
      fail("git returned an invalid changed-path record");
    }
    entries.push({ status, path });
  }
  return entries;
}

try {
  if (!repoArgument || !shaPattern.test(base ?? "") || !shaPattern.test(target ?? "")) {
    fail("usage: typescript-scope.mjs <repo> <40-char-base-sha> <40-char-target-sha>");
  }

  const repo = resolve(repoArgument);
  git(repo, "cat-file", "-e", `${base}^{commit}`);
  git(repo, "cat-file", "-e", `${target}^{commit}`);
  execFileSync("git", ["-C", repo, "merge-base", "--is-ancestor", base, target], {
    stdio: "ignore",
  });
  if (git(repo, "rev-parse", "HEAD") !== target) {
    fail("the repository checkout must be the exact target commit");
  }

  const projects = new Map();
  const directoryToName = new Map();
  for (const manifest of workspaceManifests(repo)) {
    const packageJson = readJson(manifest.path);
    const name = packageJson.name;
    if (
      typeof name !== "string" ||
      !/^(?:@[a-z0-9][a-z0-9._~-]*\/)?[a-z0-9][a-z0-9._~-]*$/.test(name)
    ) {
      fail(`workspace ${manifest.directory} has an unsafe or missing package name`);
    }
    if (projects.has(name)) fail(`duplicate workspace package name: ${name}`);
    projects.set(name, {
      directory: manifest.directory,
      packageJson,
      dependencies: new Set(),
      dependents: new Set(),
    });
    directoryToName.set(manifest.directory, name);
  }
  if (projects.size === 0) fail("no TypeScript workspace projects were discovered");

  const dependencyFields = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
  ];
  for (const [name, project] of projects) {
    for (const field of dependencyFields) {
      const dependencies = project.packageJson[field];
      if (dependencies === undefined) continue;
      if (dependencies === null || typeof dependencies !== "object" || Array.isArray(dependencies)) {
        fail(`${project.directory} has an invalid ${field} object`);
      }
      for (const dependencyName of Object.keys(dependencies)) {
        if (!projects.has(dependencyName)) continue;
        project.dependencies.add(dependencyName);
        projects.get(dependencyName).dependents.add(name);
      }
    }
  }

  const changedProjects = new Set();
  for (const entry of changedEntries(repo)) {
    if (
      entry.path === "package.json" ||
      entry.path === "pnpm-lock.yaml" ||
      entry.path === "pnpm-workspace.yaml" ||
      entry.path === ".node-version" ||
      /^tsconfig(?:\.[^/]+)?\.json$/.test(entry.path)
    ) {
      selectFull(`shared TypeScript input changed: ${entry.path}`);
    }

    const match = /^(apps|packages)\/([^/]+)(?:\/|$)/.exec(entry.path);
    if (!match) continue;
    if (entry.status === "D") {
      selectFull(`workspace path was deleted: ${entry.path}`);
    }
    const directory = `${match[1]}/${match[2]}`;
    const name = directoryToName.get(directory);
    if (!name) selectFull(`changed path does not map to a current workspace: ${entry.path}`);
    changedProjects.add(name);
  }

  if (changedProjects.size === 0) {
    process.stdout.write("none\n");
    process.exit(0);
  }

  const selected = new Set(changedProjects);
  const addClosure = (edgeName) => {
    const queue = [...selected];
    for (let index = 0; index < queue.length; index += 1) {
      const name = queue[index];
      for (const related of projects.get(name)[edgeName]) {
        if (selected.has(related)) continue;
        selected.add(related);
        queue.push(related);
      }
    }
  };

  // Test every consumer of the changed package, then include the selected consumers' prerequisites
  // so build/typecheck scripts can run from a clean checkout without relying on stale artifacts.
  addClosure("dependents");
  addClosure("dependencies");

  process.stdout.write(`filtered\n${[...selected].sort().join("\n")}\n`);
} catch (error) {
  process.stderr.write(`[typescript-scope] ${error.message}\n`);
  process.exit(1);
}
