"use client";

import { useState } from "react";
import { useLanguage } from "@/components/chrome";
import {
  OFFICIAL_DOCS_URL,
  buildAgentHandoff,
  buildClaudeCodeCommands,
  buildCodexCommands,
} from "@/lib/connect-commands";

const copy = {
  en: {
    eyebrow: "Quick setup",
    title: "Connect your coding tool",
    text: "One key for both APIs. Save the environment variables in ~/.zshrc, reload it, and run Claude Code or Codex.",
    statePill: "Setup template",
    stepOne: "Paste your key",
    stepTwo: "Copy commands",
    stepThreeClaude: "Run Claude",
    stepThreeCodex: "Run Codex",
    tabClaude: "Claude Code",
    tabCodex: "Codex · OpenAI",
    terminal: "Terminal setup",
    copyTemplate: "Copy setup template",
    terminalCopied: "Setup copied",
    agentTitle: "Using a coding agent?",
    agentText: "Give it the complete integration brief instead.",
    copyBrief: "Copy agent brief",
    briefCopied: "Brief copied",
    openDocs: "Open full docs",
    note: "Replace YOUR_SK_POOL_API_KEY with your sk-pool key — the commands save it in ~/.zshrc for new terminals.",
  },
  ru: {
    eyebrow: "Быстрая настройка",
    title: "Подключите инструмент разработки",
    text: "Один ключ для обоих API. Сохраните переменные окружения в ~/.zshrc, перезагрузите его и запустите Claude Code или Codex.",
    statePill: "Шаблон настройки",
    stepOne: "Вставьте ключ",
    stepTwo: "Скопируйте команды",
    stepThreeClaude: "Запустите Claude",
    stepThreeCodex: "Запустите Codex",
    tabClaude: "Claude Code",
    tabCodex: "Codex · OpenAI",
    terminal: "Настройка терминала",
    copyTemplate: "Скопировать шаблон",
    terminalCopied: "Настройка скопирована",
    agentTitle: "Работаете с ИИ-агентом?",
    agentText: "Передайте ему готовое техническое задание.",
    copyBrief: "Скопировать задание",
    briefCopied: "Задание скопировано",
    openDocs: "Открыть полные доки",
    note: "Замените YOUR_SK_POOL_API_KEY на ваш ключ sk-pool — команды сохранят его в ~/.zshrc для новых терминалов.",
  },
} as const;

function DockCopyButton({ value, label, copiedLabel, className }: { value: string; label: string; copiedLabel: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={`btn btn-ghost btn-sm${className ? ` ${className}` : ""}`}
      onClick={() => {
        void navigator.clipboard.writeText(value).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1600);
        });
      }}
    >
      {copied ? copiedLabel : label}
    </button>
  );
}

function TerminalCommands({ commands }: { commands: string }) {
  return <code>{commands.split("\n").map((command, index) => {
    const assignmentEnd = command.indexOf("=") + 1;
    const prefix = assignmentEnd > 0 ? command.slice(0, assignmentEnd) : command;
    const value = assignmentEnd > 0 ? command.slice(assignmentEnd) : "";
    return <span className="agent-terminal-line" key={`${index}-${command}`}>{prefix}{assignmentEnd > 0 && <wbr />}{value || " "}</span>;
  })}</code>;
}

export function QuickConnectDock({ defaultExpanded = false }: { defaultExpanded?: boolean }) {
  const { language } = useLanguage();
  const t = copy[language];
  const [expanded, setExpanded] = useState(defaultExpanded);
  const [surface, setSurface] = useState<"claude" | "codex">("claude");
  const terminalCommands = surface === "claude" ? buildClaudeCodeCommands() : buildCodexCommands();
  const handoff = buildAgentHandoff({ language });

  return (
    <aside className={`agent-connect-dock${expanded ? " is-open" : ""}`} aria-labelledby="agent-connect-title">
      <button className="agent-connect-summary" type="button" aria-expanded={expanded} aria-controls="agent-connect-body" onClick={() => setExpanded((current) => !current)}>
        <span className="agent-connect-icon" aria-hidden="true">&gt;_</span>
        <span className="agent-connect-main"><span>{t.eyebrow}</span><strong id="agent-connect-title">{t.title}</strong><small>{t.text}</small></span>
        <span className="agent-connect-state"><i />{t.statePill}</span>
        <span className="agent-connect-chevron" aria-hidden="true">⌄</span>
      </button>
      {expanded && (
        <div className="agent-connect-body" id="agent-connect-body">
          <div className="agent-connect-path" aria-label={t.eyebrow}>
            <span><b>1</b>{t.stepOne}</span><i>→</i>
            <span><b>2</b>{t.stepTwo}</span><i>→</i>
            <span><b>3</b>{surface === "claude" ? t.stepThreeClaude : t.stepThreeCodex}</span>
          </div>
          <div className="agent-connect-tabs" role="group" aria-label={t.terminal}>
            <button type="button" className={surface === "claude" ? "active" : ""} aria-pressed={surface === "claude"} onClick={() => setSurface("claude")}>{t.tabClaude}</button>
            <button type="button" className={surface === "codex" ? "active" : ""} aria-pressed={surface === "codex"} onClick={() => setSurface("codex")}>{t.tabCodex}</button>
          </div>
          <div className="agent-terminal" aria-label={t.terminal}>
            <div className="agent-terminal-head">
              <span><i /><i /><i />{t.terminal}</span>
              <DockCopyButton value={terminalCommands} className="agent-connect-copy" label={t.copyTemplate} copiedLabel={t.terminalCopied} />
            </div>
            <pre><TerminalCommands commands={terminalCommands} /></pre>
          </div>
          <div className="agent-connect-footer">
            <div><strong>{t.agentTitle}</strong><span>{t.agentText}</span></div>
            <div className="agent-connect-footer-actions">
              <DockCopyButton value={handoff} label={t.copyBrief} copiedLabel={t.briefCopied} />
              <a className="btn btn-ghost btn-sm" href={OFFICIAL_DOCS_URL} target="_blank" rel="noreferrer">{t.openDocs} ↗</a>
            </div>
          </div>
          <p className="agent-connect-note"><span aria-hidden="true">ⓘ</span><span>{t.note}</span></p>
        </div>
      )}
    </aside>
  );
}
