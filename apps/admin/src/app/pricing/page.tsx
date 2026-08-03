"use client";

import { useCallback, useState } from "react";
import { LoadingGrid, Modal, PageHead, Pill, SectionHeader, TableCard } from "@/components/ui";
import { api, send } from "@/lib/api";
import { dialog } from "@/lib/dialog";
import { toast } from "@/lib/toast";
import { usePoll } from "@/lib/usePoll";
import {
  canonicalizePricingRules,
  ManagedPolicyEditor,
  type ManagedPolicyView,
  type PricingCatalogView,
  type PricingRule,
} from "../business/policy-editor";
import { PANEL_REASON } from "../business/utils";
import { PricingReleaseActivationControl } from "./activation-control";
import { PricingStage8CaptureControl } from "./stage8-capture-control";

interface PricingData {
  catalog: PricingCatalogView | null;
  globalPolicy: ManagedPolicyView | null;
  services: ManagedPolicyView[];
  serviceCatalogs: Record<string, PricingCatalogView>;
}

type SwitchState = Array<{
  providerId: string;
  masterEnabled: boolean;
  productEnabled: boolean;
  b2cEnabled: boolean;
  b2bEnabled: boolean;
}>;

const errorText = (error: unknown): string => error instanceof Error ? error.message : String(error);

async function loadPricing(): Promise<PricingData> {
  const [catalog, globalPolicy, services] = await Promise.all([
    api<PricingCatalogView>("/admin/pricing-catalog").catch(() => null),
    api<ManagedPolicyView>("/admin/pricing-policies/global-b2c").catch(() => null),
    api<{ policies: ManagedPolicyView[] }>("/admin/service-policies").catch(() => ({ policies: [] })),
  ]);
  const productIds = [...new Set(services.policies.map((policy) => policy.productId))];
  const loadedCatalogs = await Promise.all(productIds.map(async (productId) => {
    if (catalog?.productId === productId) return [productId, catalog] as const;
    const serviceCatalog = await api<PricingCatalogView>(
      `/admin/pricing-catalog?product_id=${encodeURIComponent(productId)}`,
    ).catch(() => null);
    return serviceCatalog ? [productId, serviceCatalog] as const : null;
  }));
  return {
    catalog,
    globalPolicy,
    services: services.policies,
    serviceCatalogs: Object.fromEntries(loadedCatalogs.filter((entry) => entry !== null)),
  };
}

function GlobalPolicyEditor(props: {
  catalog: PricingCatalogView;
  policy: ManagedPolicyView;
  onChanged: () => void;
}) {
  const [rules, setRules] = useState<PricingRule[]>(() => canonicalizePricingRules(props.policy.rules));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    if (rules.length === 0) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await send<ManagedPolicyView>("/admin/pricing-policies/global-b2c", "PATCH", {
        expectedVersion: props.policy.currentVersion,
        reason: PANEL_REASON,
        rules: canonicalizePricingRules(rules),
      });
      setRules(canonicalizePricingRules(updated.rules));
      toast(`Global B2C policy v${updated.currentVersion} сохранена; ожидаем exact ACK.`);
      props.onChanged();
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="form-card">
      <ManagedPolicyEditor
        catalog={props.catalog}
        policy={props.policy}
        rules={rules}
        onRulesChange={setRules}
        allowTrack
        segment="b2c"
        disabled={saving}
      />
      {error ? <div className="policy-rule-count bad">{error}</div> : null}
      <div className="dlg-actions">
        <button className="btn" disabled={saving || rules.length === 0} onClick={() => void save()}>
          {saving ? "сохраняем…" : "сохранить Global B2C replacement policy"}
        </button>
      </div>
    </div>
  );
}

function ProviderSwitchEditor(props: { catalog: PricingCatalogView; onChanged: () => void }) {
  const initial = (): SwitchState => props.catalog.providers.map((provider) => ({
    providerId: provider.providerId,
    masterEnabled: provider.masterEnabled,
    productEnabled: provider.productEnabled,
    b2cEnabled: provider.b2cEnabled,
    b2bEnabled: provider.b2bEnabled,
  }));
  const [providers, setProviders] = useState<SwitchState>(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggle = (providerId: string, field: Exclude<keyof SwitchState[number], "providerId">) => {
    setProviders((current) => current.map((provider) => provider.providerId === providerId
      ? { ...provider, [field]: !provider[field] }
      : provider));
  };

  const save = async () => {
    const masterChanged = providers.some((provider) => {
      const current = props.catalog.providers.find((candidate) => candidate.providerId === provider.providerId);
      return current && current.masterEnabled !== provider.masterEnabled;
    });
    const disabledGates = providers.flatMap((provider) => {
      const current = props.catalog.providers.find((candidate) => candidate.providerId === provider.providerId);
      if (!current) return [];
      return (["productEnabled", "b2cEnabled", "b2bEnabled"] as const)
        .filter((field) => current[field] && !provider[field])
        .map((field) => `${provider.providerId}:${field.replace("Enabled", "")}`);
    });
    if (masterChanged || disabledGates.length > 0) {
      const impact = [
        masterChanged ? "Изменяется аварийный master switch." : "",
        disabledGates.length > 0 ? `Будут выключены: ${disabledGates.join(", ")}.` : "",
        "Изменение versioned и не удаляет policy rules.",
      ].filter(Boolean).join(" ");
      const confirmed = await dialog({
        title: masterChanged ? "Подтвердить master/provider gates" : "Отключить новые provider admissions",
        message: impact,
        confirmLabel: masterChanged ? "Сохранить gates" : "Отключить",
        danger: true,
      });
      if (!confirmed) return;
    }
    setSaving(true);
    setError(null);
    try {
      const updated = await send<PricingCatalogView>("/admin/provider-switches", "PATCH", {
        expectedGeneration: props.catalog.switchGeneration,
        reason: PANEL_REASON,
        providers,
      });
      setProviders(updated.providers.map((provider) => ({
        providerId: provider.providerId,
        masterEnabled: provider.masterEnabled,
        productEnabled: provider.productEnabled,
        b2cEnabled: provider.b2cEnabled,
        b2bEnabled: provider.b2bEnabled,
      })));
      toast(`Provider switches g${updated.switchGeneration} сохранены; ожидаем engine ACK.`);
      props.onChanged();
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="form-card">
      <div className="policy-meta">
        <Pill kind={props.catalog.switchSyncState === "confirmed" ? "ok" : props.catalog.switchSyncState === "dead" ? "bad" : "warn"}>
          g{props.catalog.switchGeneration} · {props.catalog.switchSyncState}
        </Pill>
        <code>{props.catalog.switchDigest}</code>
        {props.catalog.switchLastError ? <span>{props.catalog.switchLastError}</span> : null}
      </div>
      <div className="policy-switch-grid">
        {providers.map((provider) => (
          <div className="policy-switch-row" key={provider.providerId}>
            <b>{provider.providerId}</b>
            <label className="master-switch">
              <input type="checkbox" checked={provider.masterEnabled} disabled={saving} onChange={() => toggle(provider.providerId, "masterEnabled")} />
              master / emergency
            </label>
            <label>
              <input type="checkbox" checked={provider.productEnabled} disabled={saving} onChange={() => toggle(provider.providerId, "productEnabled")} />
              product
            </label>
            <label>
              <input type="checkbox" checked={provider.b2cEnabled} disabled={saving} onChange={() => toggle(provider.providerId, "b2cEnabled")} />
              B2C
            </label>
            <label>
              <input type="checkbox" checked={provider.b2bEnabled} disabled={saving} onChange={() => toggle(provider.providerId, "b2bEnabled")} />
              B2B
            </label>
          </div>
        ))}
      </div>
      {error ? <div className="policy-rule-count bad">{error}</div> : null}
      <div className="dlg-actions">
        <button className="btn" disabled={saving || providers.length === 0} onClick={() => void save()}>
          {saving ? "сохраняем…" : "сохранить новую switch generation"}
        </button>
      </div>
    </div>
  );
}

export default function PricingPage() {
  const { data, refresh } = usePoll("managed-pricing", loadPricing);
  const [service, setService] = useState<ManagedPolicyView | null>(null);
  const [serviceRules, setServiceRules] = useState<PricingRule[]>([]);
  const [serviceSaving, setServiceSaving] = useState(false);
  const [serviceError, setServiceError] = useState<string | null>(null);

  const openService = useCallback((policy: ManagedPolicyView) => {
    setService(policy);
    setServiceRules(canonicalizePricingRules(policy.rules));
    setServiceError(null);
  }, []);

  const saveService = async () => {
    if (!service || serviceRules.length === 0) return;
    setServiceSaving(true);
    setServiceError(null);
    try {
      const updated = await send<ManagedPolicyView>(
        `/admin/service-policies/${encodeURIComponent(service.ownerId)}?product_id=${encodeURIComponent(service.productId)}`,
        "PATCH",
        {
          expectedVersion: service.currentVersion,
          reason: PANEL_REASON,
          rules: canonicalizePricingRules(serviceRules),
        },
      );
      setService(updated);
      setServiceRules(canonicalizePricingRules(updated.rules));
      toast(`Service policy ${updated.ownerId} v${updated.currentVersion} сохранена; ожидаем exact ACK.`);
      refresh();
    } catch (cause) {
      setServiceError(errorText(cause));
    } finally {
      setServiceSaving(false);
    }
  };

  if (!data) {
    return <><PageHead title="Pricing policies" sub="загружаем authority views" /><LoadingGrid /></>;
  }
  const serviceCatalog = service ? data.serviceCatalogs[service.productId] ?? null : null;

  return (
    <>
      <PageHead
        title="Pricing policies"
        sub="Global B2C, provider gates и explicit service policies"
        badge={data.catalog ? <Pill kind="ok">catalog g{data.catalog.catalogGeneration}</Pill> : <Pill kind="bad">foundation missing</Pill>}
      />

      <PricingStage8CaptureControl />

      <PricingReleaseActivationControl />

      <SectionHeader title="Provider switches" sub="master, product, B2C и B2B — независимые gates" />
      {data.catalog ? (
        <ProviderSwitchEditor key={data.catalog.switchGeneration} catalog={data.catalog} onChanged={refresh} />
      ) : (
        <div className="policy-rule-count bad">Catalog/switch foundation ещё не материализован.</div>
      )}

      <SectionHeader title="Global B2C policy" sub="track и точные static overrides" />
      {data.catalog && data.globalPolicy ? (
        <GlobalPolicyEditor
          key={data.globalPolicy.currentVersion}
          catalog={data.catalog}
          policy={data.globalPolicy}
          onChanged={refresh}
        />
      ) : (
        <div className="policy-rule-count bad">Global B2C policy отсутствует до Stage 5 materialization.</div>
      )}

      <SectionHeader title="Service policies" sub="только reviewed explicit assignments; создание по имени запрещено" />
      <TableCard>
        <table>
          <thead><tr><th className="left">owner</th><th>product</th><th>source</th><th>targets</th><th>sync</th><th /></tr></thead>
          <tbody>
            {data.services.length ? data.services.map((policy) => {
              const confirmed = policy.targets.length > 0 && policy.targets.every((target) => target.syncState === "confirmed");
              return (
                <tr key={policy.policyId}>
                  <td className="left">
                    <b>{policy.ownerId}</b>
                    <div className="sub mono">{policy.policyId}</div>
                    {policy.servicePurpose ? <div className="sub">{policy.servicePurpose}</div> : null}
                    {policy.serviceResponsible ? <div className="sub">ответственный: {policy.serviceResponsible}</div> : null}
                  </td>
                  <td><code>{policy.productId}</code></td>
                  <td>v{policy.currentVersion}<div className="sub">{policy.rules.length} правил</div></td>
                  <td>{policy.targets.length}</td>
                  <td><Pill kind={confirmed ? "ok" : "warn"}>{confirmed ? "confirmed" : "pending"}</Pill></td>
                  <td><button className="btn" onClick={() => openService(policy)}>открыть</button></td>
                </tr>
              );
            }) : <tr><td className="empty" colSpan={6}>Service assignments ещё не материализованы; UI не угадывает их автоматически.</td></tr>}
          </tbody>
        </table>
      </TableCard>

      <Modal
        open={service !== null}
        title={service ? `Service policy · ${service.ownerId}` : "Service policy"}
        wide
        onClose={() => setService(null)}
      >
        {service && serviceCatalog ? (
          <ManagedPolicyEditor
            catalog={serviceCatalog}
            policy={service}
            rules={serviceRules}
            onRulesChange={setServiceRules}
            segment="service"
            disabled={serviceSaving}
          />
        ) : null}
        {serviceError ? <div className="policy-rule-count bad">{serviceError}</div> : null}
        <div className="dlg-actions">
          <button className="btn ghost" onClick={() => setService(null)}>Закрыть</button>
          <button className="btn" disabled={!serviceCatalog || serviceSaving || serviceRules.length === 0} onClick={() => void saveService()}>
            {serviceSaving ? "сохраняем…" : "сохранить replacement policy"}
          </button>
        </div>
      </Modal>
    </>
  );
}
