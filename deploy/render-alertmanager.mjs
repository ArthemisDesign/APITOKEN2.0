#!/usr/bin/env node

import fs from "node:fs";

const [templatePath, workerEnvironmentPath, monitoringEnvironmentPath, outputPath] = process.argv.slice(2);
if (!templatePath || !workerEnvironmentPath || !monitoringEnvironmentPath || !outputPath) {
  process.stderr.write("usage: render-alertmanager.mjs <template> <worker-env> <monitoring-env> <output>\n");
  process.exit(2);
}

function parseEnvironment(path) {
  const values = new Map();
  for (const [index, rawLine] of fs.readFileSync(path, "utf8").split(/\r?\n/u).entries()) {
    const line = rawLine.trim();
    if (line === "" || line.startsWith("#")) continue;
    const separator = line.indexOf("=");
    if (separator < 1) throw new Error(`${path}:${index + 1}: invalid environment assignment`);
    const key = line.slice(0, separator).trim();
    if (!/^[A-Z][A-Z0-9_]*$/u.test(key)) throw new Error(`${path}:${index + 1}: invalid key`);
    if (values.has(key)) throw new Error(`${path}:${index + 1}: duplicate ${key}`);
    let value = line.slice(separator + 1).trim();
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    values.set(key, value);
  }
  return values;
}

function required(values, name, path) {
  const value = values.get(name);
  if (!value) throw new Error(`${path}: ${name} is required`);
  return value;
}

const worker = parseEnvironment(workerEnvironmentPath);
const monitoring = parseEnvironment(monitoringEnvironmentPath);
const host = required(worker, "SMTP_HOST", workerEnvironmentPath);
const port = required(worker, "SMTP_PORT", workerEnvironmentPath);
if (!/^\d{1,5}$/u.test(port) || Number(port) < 1 || Number(port) > 65_535) {
  throw new Error(`${workerEnvironmentPath}: SMTP_PORT is invalid`);
}

const replacements = new Map([
  ["__SMTP_SMARTHOST__", `${host}:${port}`],
  ["__SMTP_FROM__", required(worker, "EMAIL_FROM", workerEnvironmentPath)],
  ["__SMTP_USERNAME__", required(worker, "SMTP_USERNAME", workerEnvironmentPath)],
  ["__SMTP_PASSWORD__", required(worker, "SMTP_PASSWORD", workerEnvironmentPath)],
  ["__ALERT_EMAIL_TO__", required(monitoring, "ALERT_EMAIL_TO", monitoringEnvironmentPath)],
]);

let rendered = fs.readFileSync(templatePath, "utf8");
for (const [placeholder, value] of replacements) {
  if (!rendered.includes(placeholder)) throw new Error(`${templatePath}: missing ${placeholder}`);
  rendered = rendered.replaceAll(placeholder, JSON.stringify(value));
}
if (/__[A-Z0-9_]+__/u.test(rendered)) throw new Error(`${templatePath}: unresolved placeholder`);
fs.writeFileSync(outputPath, rendered, { mode: 0o600 });
