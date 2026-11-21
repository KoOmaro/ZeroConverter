import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { LOCALES, type LanguagePreference } from "../../i18n/locales";
import { getLanguagePreference, setLanguagePreference } from "../../i18n";
import { Bell, Check, Film, Folder, FolderPlus, Info, Monitor, Palette, Settings2, X } from "lucide-react";
import { useVideoEngine } from "../../application/videoEngine";
import {
  THEMES,
  themeManager,
  type DisplayPreferences,
  type ThemeInfo,
  type ThemeMode,
  type UiScale,
} from "../../application/themeManager";
import { useRenameConvert } from "../../application/renameConvert";
import { NOTIFICATION_SOUNDS, useNotificationPrefs } from "../../application/notifications";
import { useAppUpdate } from "../../application/appUpdate";

type SectionId = "general" | "video" | "appearance" | "notifications" | "about";

const SECTIONS: Array<{ id: SectionId; labelKey: string; icon: typeof Bell }> = [
  { id: "general", labelKey: "settings.general", icon: Settings2 },
  { id: "video", labelKey: "settings.videoSection", icon: Film },
  { id: "appearance", labelKey: "settings.appearance", icon: Palette },
  { id: "notifications", labelKey: "settings.notifications", icon: Bell },
  { id: "about", labelKey: "settings.about", icon: Info },
];

interface SettingsDialogProps {
  open: boolean;
  onClose: () => void;
  version: string;
  /** Open at this page (a rename that needs FFmpeg asks for "video"). */
  request?: { section: string } | null;
}

export const SettingsDialog: React.FC<SettingsDialogProps> = ({ open, onClose, version, request }) => {
  const { t } = useTranslation();
  const ref = useRef<HTMLDialogElement>(null);
  const [section, setSection] = useState<SectionId>("general");

  useEffect(() => {
    const wanted = SECTIONS.find((s) => s.id === request?.section);
    if (wanted) setSection(wanted.id);
  }, [request]);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      onClose={onClose}
      onClick={(e) => e.target === ref.current && onClose()}
      className="zc-dialog m-auto h-[min(560px,86vh)] w-[min(820px,92vw)] overflow-hidden rounded-panel border border-border-subtle bg-surface-1 p-0 text-text-primary"
      aria-labelledby="settings-title"
    >
      <div className="grid h-full grid-cols-[220px_1fr]">
        <nav className="flex flex-col gap-0.5 border-e border-border-layout bg-rail p-3" aria-label={t("settings.sections")}>
          <h2 id="settings-title" className="px-2.5 pb-3 pt-1.5 text-[13px] font-semibold text-text-secondary">
            {t("settings.title")}
          </h2>
          {SECTIONS.map(({ id, labelKey, icon: Icon }) => (
            <button
              key={id}
              type="button"
              onClick={() => setSection(id)}
              aria-current={section === id ? "page" : undefined}
              className={`flex h-9 items-center gap-2.5 rounded-control px-2.5 text-[13px] transition-colors duration-150 ${
                section === id
                  ? "bg-surface-2 font-medium text-text-primary"
                  : "text-text-secondary hover:bg-surface-2/60 hover:text-text-primary"
              }`}
            >
              <Icon size={16} strokeWidth={1.8} aria-hidden="true" />
              {t(labelKey)}
            </button>
          ))}
        </nav>

        <div className="relative min-h-0 overflow-y-auto px-8 pb-8 pt-6">
          <button
            type="button"
            onClick={onClose}
            data-press="icon"
            className="zc-icon-btn absolute end-4 top-4"
            aria-label={t("settings.close")}
          >
            <X size={16} strokeWidth={1.75} aria-hidden="true" />
          </button>
          {section === "general" && <GeneralSection />}
          {section === "video" && <VideoSection />}
          {section === "appearance" && <AppearanceSection />}
          {section === "notifications" && <NotificationsSection />}
          {section === "about" && <AboutSection version={version} />}
        </div>
      </div>
    </dialog>
  );
};

const SectionTitle: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <h3 className="mb-5 text-[18px] font-semibold tracking-[-0.01em]">{children}</h3>
);

const Row: React.FC<{ title: string; description?: string; control: React.ReactNode; htmlFor?: string }> = ({
  title,
  description,
  control,
  htmlFor,
}) => (
  <div className="flex items-center justify-between gap-6 border-b border-border-layout py-3.5 last:border-b-0">
    <div className="min-w-0">
      <label htmlFor={htmlFor} className="block text-[13px] font-medium text-text-primary">{title}</label>
      {description && <p className="mt-0.5 text-[12px] leading-relaxed text-text-tertiary">{description}</p>}
    </div>
    <div className="shrink-0">{control}</div>
  </div>
);

const Switch: React.FC<{ id: string; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }> = ({
  id,
  checked,
  onChange,
  disabled,
}) => (
  <button
    id={id}
    type="button"
    role="switch"
    aria-checked={checked}
    disabled={disabled}
    onClick={() => onChange(!checked)}
    className={`relative h-5 w-9 rounded-full transition-colors duration-150 disabled:opacity-50 ${checked ? "bg-accent" : "bg-surface-3"}`}
  >
    <span
      className={`absolute start-0 top-0.5 h-4 w-4 rounded-full bg-text-primary transition-transform duration-150 ${checked ? "translate-x-[18px] rtl:-translate-x-[18px]" : "translate-x-0.5 rtl:-translate-x-0.5"}`}
    />
  </button>
);

const select =
  "zc-field !w-auto";

const GeneralSection: React.FC = () => {
  const rename = useRenameConvert();
  const { t } = useTranslation();
  const [language, setLanguage] = useState<LanguagePreference>(getLanguagePreference);
  return (
    <section>
      <SectionTitle>{t("settings.general")}</SectionTitle>
      <Row
        htmlFor="set-language"
        title={t("settings.languageTitle")}
        description={t("settings.languageDesc")}
        control={
          <select
            id="set-language"
            value={language}
            onChange={(e) => {
              const next = e.target.value as LanguagePreference;
              setLanguage(next);
              setLanguagePreference(next);
            }}
            className={select}
          >
            <option value="system">{t("settings.languageSystem")}</option>
            {LOCALES.map((l) => (
              <option key={l.code} value={l.code} lang={l.code} dir={l.dir}>{l.name}</option>
            ))}
          </select>
        }
      />
      <Row
        htmlFor="set-rename"
        title={t("settings.renameTitle")}
        description={t("settings.renameDesc", { from: "photo.png", to: "photo.webp", folder: "Holiday.webp" })}
        control={<Switch id="set-rename" checked={!!rename.enabled} disabled={rename.enabled === null} onChange={rename.setEnabled} />}
      />
      <div className={`border-b border-border-layout py-3.5 transition-opacity duration-150 ${rename.enabled === false ? "opacity-50" : ""}`}>
        <h4 className="text-[13px] font-medium text-text-primary">{t("settings.suffixTitle")}</h4>
        <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-[12px]">
          {[
            ["photo.webp+", t("settings.suffixQuality")],
            ["photo.webp&", t("settings.suffixKeep")],
            ["photo.webp@1080", t("settings.suffixSize")],
          ].map(([example, meaning]) => (
            <React.Fragment key={example}>
              <dt className="font-mono text-text-secondary" dir="ltr">{example}</dt>
              <dd className="text-text-tertiary">{meaning}</dd>
            </React.Fragment>
          ))}
        </dl>
        <p className="mt-2 text-[12px] text-text-tertiary">{t("settings.suffixCombine", { example: "photo.webp&@1080" })}</p>
      </div>
      <div className={`border-b border-border-layout py-3.5 transition-opacity duration-150 ${rename.enabled === false ? "opacity-50" : ""}`}>
        <div className="flex items-start justify-between gap-6">
          <div className="min-w-0">
            <h4 className="text-[13px] font-medium text-text-primary">{t("settings.foldersTitle")}</h4>
            <p className="mt-0.5 text-[12px] leading-relaxed text-text-tertiary">
              {t("settings.foldersDesc")}
            </p>
          </div>
          <button type="button" data-press className="zc-btn zc-btn-sm zc-btn-secondary shrink-0" onClick={rename.addFolder}>
            <FolderPlus size={14} strokeWidth={1.75} aria-hidden="true" />
            {t("settings.addFolder")}
          </button>
        </div>
        <ul className="mt-3 flex flex-col" aria-label={t("settings.foldersTitle")}>
          {rename.folders.map((folder) => (
            <li key={folder.path} className="flex h-8 items-center gap-2.5 rounded-control px-2 text-[12px] hover:bg-surface-2/60">
              <Folder size={14} strokeWidth={1.75} aria-hidden="true" className="shrink-0 text-text-tertiary" />
              <span className="min-w-0 flex-1 truncate text-text-secondary" title={folder.path} dir="ltr" style={{ textAlign: "start" }}>
                {folder.path}
              </span>
              {folder.builtin ? (
                <span className="shrink-0 text-[11px] text-text-tertiary">{t("settings.builtIn")}</span>
              ) : (
                <button
                  type="button"
                  data-press="icon"
                  className="zc-icon-btn !h-6 !w-6 shrink-0"
                  onClick={() => rename.removeFolder(folder.path)}
                  aria-label={t("settings.stopWatchingPath", { path: folder.path })}
                  title={t("settings.stopWatching")}
                >
                  <X size={13} strokeWidth={1.75} aria-hidden="true" />
                </button>
              )}
            </li>
          ))}
        </ul>
        {rename.folderError && (
          <p className="mt-2 text-[12px] text-danger" role="alert">
            {rename.folderError}
          </p>
        )}
      </div>
      <Row
        htmlFor="set-context-menu"
        title={t("settings.contextTitle")}
        description={t("settings.contextDesc")}
        control={<Switch id="set-context-menu" checked={!!rename.contextMenu} disabled={rename.contextMenu === null} onChange={rename.setContextMenu} />}
      />
      <Row
        htmlFor="set-autostart"
        title={t("settings.autostartTitle")}
        description={t("settings.autostartDesc")}
        control={<Switch id="set-autostart" checked={!!rename.autostart} disabled={rename.autostart === null} onChange={rename.setAutostart} />}
      />
    </section>
  );
};

const mb = (bytes: number) => `${Math.max(1, Math.round(bytes / (1024 * 1024)))} MB`;

const VideoSection: React.FC = () => {
  const { t } = useTranslation();
  const { status, progress, error, install, cancel } = useVideoEngine();
  const ready = !!status?.available;
  const where =
    status?.source === "system"
      ? t("settings.ffmpegSystem", { path: status.path ? ` (${status.path})` : "" })
      : status?.source === "override"
        ? t("settings.ffmpegOverride", { variable: "ZC_FFMPEG_DIR", path: status.path ? ` (${status.path})` : "" })
        : t("settings.ffmpegManaged");
  const percent = progress && progress.total > 0 ? Math.min(100, Math.round((progress.done / progress.total) * 100)) : 0;
  const stage =
    progress?.phase === "verify" ? t("settings.stageCheck") : progress?.phase === "extract" ? t("settings.stageUnpack") : t("settings.stageDownload");

  return (
    <section>
      <SectionTitle>{t("settings.videoSection")}</SectionTitle>
      <p className="mb-4 max-w-[60ch] text-[12px] leading-relaxed text-text-tertiary">
        {t("settings.videoIntro")}
      </p>
      <Row
        title="FFmpeg"
        description={
          status === null
            ? t("common.checking")
            : ready
              ? t("settings.ffmpegReady", { version: status.version ?? t("common.ready"), where })
              : status.canInstall
                ? t("settings.ffmpegMissingInstall", { size: mb(status.downloadBytes), host: new URL(status.downloadUrl).host, license: status.license })
                : t("settings.ffmpegMissing")
        }
        control={
          ready || !status?.canInstall ? (
            ready ? (
              <span className="flex items-center gap-1.5 text-[12px] text-text-secondary">
                <Check size={14} strokeWidth={2} aria-hidden="true" className="text-accent-strong" />
                {t("common.ready")}
              </span>
            ) : null
          ) : progress ? (
            <button type="button" data-press className="zc-btn zc-btn-sm zc-btn-secondary" onClick={cancel} disabled={progress.phase === "verify"}>
              {t("common.cancel")}
            </button>
          ) : (
            <button type="button" data-press className="zc-btn zc-btn-sm zc-btn-primary" onClick={install}>
              {t("common.download")}
            </button>
          )
        }
      />
      {progress && (
        <div className="py-3" aria-live="polite">
          <div className="flex items-baseline justify-between text-[12px] text-text-secondary">
            <span>{stage}</span>
            <span className="tabular-nums">{percent}%</span>
          </div>
          <div
            className="mt-2 h-1 overflow-hidden rounded-full bg-surface-3"
            role="progressbar"
            aria-label={stage}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent}
          >
            <div className="h-full bg-accent-strong transition-[width] duration-200 ease-out" style={{ width: `${percent}%` }} />
          </div>
        </div>
      )}
      {error && (
        <p className="mt-2 text-[12px] text-danger" role="alert">
          {error}
        </p>
      )}
    </section>
  );
};

function useDisplayPrefs(): DisplayPreferences {
  const [prefs, setPrefs] = useState(themeManager.get());
  useEffect(() => themeManager.subscribe(setPrefs), []);
  return prefs;
}

const ThemeCard: React.FC<{ theme: ThemeInfo | null; active: boolean; onPick: () => void }> = ({ theme, active, onPick }) => {
  const { t } = useTranslation();
  const [shell, surface, accent] = theme?.swatch ?? ["#0E1012", "#FAFBFC", "#2D5C82"];
  return (
    <button
      type="button"
      onClick={onPick}
      aria-pressed={active}
      className={`group overflow-hidden rounded-panel border text-start transition-colors duration-150 ${
        active ? "border-accent-strong bg-accent-soft" : "border-border-subtle hover:border-border-strong"
      }`}
    >
      <div className="relative h-[72px] p-2" style={{ background: theme ? shell : `linear-gradient(135deg, ${shell} 50%, #DDE2E8 50%)` }}>
        <div className="h-full w-[62%] rounded-control p-2" style={{ background: surface }}>
          <div className="mb-1.5 h-1.5 w-3/4 rounded-full" style={{ background: accent, opacity: 0.85 }} />
          <div className="h-1.5 w-1/2 rounded-full opacity-40" style={{ background: accent }} />
        </div>
        <div className="absolute bottom-2 right-2 h-1.5 w-6 rounded-full" style={{ background: accent }} />
      </div>
      <div className="flex items-center gap-1.5 px-2.5 py-2">
        {!theme && <Monitor size={13} strokeWidth={1.8} aria-hidden="true" className="text-text-tertiary" />}
        <span className="flex-1 text-[12px] font-medium">{theme ? t(`theme.${theme.id}`) : t("settings.themeSystem")}</span>
        {active && <Check size={13} strokeWidth={2} aria-hidden="true" className="text-accent-strong" />}
      </div>
    </button>
  );
};

const AppearanceSection: React.FC = () => {
  const { t } = useTranslation();
  const prefs = useDisplayPrefs();
  const pick = (theme: ThemeMode) => themeManager.update({ theme });
  return (
    <section>
      <SectionTitle>{t("settings.appearance")}</SectionTitle>
      <Row
        htmlFor="set-scale"
        title={t("settings.scale")}
        control={
          <select
            id="set-scale"
            value={prefs.scale}
            onChange={(e) => themeManager.update({ scale: Number(e.target.value) as UiScale })}
            className={select}
          >
            <option value={100}>100%</option>
            <option value={110}>110%</option>
            <option value={125}>125%</option>
          </select>
        }
      />
      <Row
        htmlFor="set-motion"
        title={t("settings.motion")}
        description={t("settings.motionDesc")}
        control={
          <select
            id="set-motion"
            value={prefs.motion}
            onChange={(e) => themeManager.update({ motion: e.target.value as DisplayPreferences["motion"] })}
            className={select}
          >
            <option value="system">{t("settings.motionSystem")}</option>
            <option value="reduce">{t("settings.motionOn")}</option>
          </select>
        }
      />
      <h4 className="mb-3 mt-7 text-[13px] font-semibold">{t("settings.theme")}</h4>
      <div className="grid grid-cols-3 gap-3">
        <ThemeCard theme={null} active={prefs.theme === "system"} onPick={() => pick("system")} />
        {THEMES.map((t) => (
          <ThemeCard key={t.id} theme={t} active={prefs.theme === t.id} onPick={() => pick(t.id)} />
        ))}
      </div>
      <p className="mt-3 text-[12px] text-text-tertiary">{t("settings.themeNote", { dark: t("theme.graphite-steel"), light: t("theme.graphite-light") })}</p>
    </section>
  );
};

const NotificationsSection: React.FC = () => {
  const { t } = useTranslation();
  const { prefs, update, preview } = useNotificationPrefs();
  if (!prefs) return <SectionTitle>{t("settings.notifications")}</SectionTitle>;
  return (
    <section>
      <SectionTitle>{t("settings.notifications")}</SectionTitle>
      <Row
        htmlFor="set-notify"
        title={t("settings.notifyTitle")}
        description={t("settings.notifyDesc")}
        control={<Switch id="set-notify" checked={prefs.enabled} onChange={(enabled) => update({ enabled })} />}
      />
      <Row
        htmlFor="set-sound"
        title={t("settings.soundTitle")}
        description={t("settings.soundDesc")}
        control={<Switch id="set-sound" checked={prefs.sound} disabled={!prefs.enabled} onChange={(sound) => update({ sound })} />}
      />
      <Row
        htmlFor="set-sound-id"
        title={t("settings.soundPick")}
        control={
          <select
            id="set-sound-id"
            value={prefs.sound_id}
            disabled={!prefs.enabled || !prefs.sound}
            onChange={(e) => update({ sound_id: e.target.value })}
            className={select}
          >
            {NOTIFICATION_SOUNDS.map((s) => (
              <option key={s.id} value={s.id}>{t(`sound.${s.id}`)}</option>
            ))}
          </select>
        }
      />
      <Row
        htmlFor="set-volume"
        title={t("settings.volume")}
        control={
          <div className="flex items-center gap-3">
            <input
              id="set-volume"
              type="range"
              min={0}
              max={100}
              step={5}
              value={prefs.volume}
              disabled={!prefs.enabled || !prefs.sound}
              onChange={(e) => update({ volume: Number(e.target.value) })}
              className="zc-range w-36"
              style={{ "--fill": `${prefs.volume}%` } as React.CSSProperties}
            />
            <span className="w-8 text-end font-mono text-[12px] tabular-nums text-text-secondary">{prefs.volume}</span>
            <button
              type="button"
              onClick={preview}
              disabled={!prefs.enabled}
              className="zc-btn zc-btn-secondary"
            >
              {t("settings.preview")}
            </button>
          </div>
        }
      />
    </section>
  );
};

const AboutSection: React.FC<{ version: string }> = ({ version }) => {
  const { t } = useTranslation();
  const update = useAppUpdate();
  const checkLabel =
    update.check === "checking"
      ? t("settings.updateChecking")
      : update.check === "latest"
        ? t("settings.updateLatest")
        : update.check === "failed"
          ? t("settings.updateFailed")
          : t("settings.updateCheck");
  return (
  <section>
    <SectionTitle>{t("settings.about")}</SectionTitle>
    <div className="flex items-center gap-4">
      <img src="/zeroconverter-mark.svg" alt="" width={48} height={48} />
      <div>
        <p className="font-brand text-[17px] font-semibold">ZeroConverter</p>
        <p className="text-[12px] text-text-tertiary">{t("settings.version", { version })}</p>
      </div>
    </div>
    <p className="mt-6 max-w-[52ch] text-[13px] leading-relaxed text-text-secondary">
      {t("settings.aboutBody")}
    </p>
    <p className="mt-4 max-w-[52ch] text-[12px] leading-relaxed text-text-tertiary">
      {t("settings.aboutLicense")}
    </p>
    <div className="mt-6">
      <Row
        htmlFor="set-auto-update"
        title={t("settings.updatesTitle")}
        description={
          update.available
            ? t("settings.updateAvailable", { version: update.available })
            : t("settings.updateDefault")
        }
        control={
          <div className="flex items-center gap-3">
            {update.available ? (
              <button
                type="button"
                data-press
                className="zc-btn zc-btn-sm zc-btn-primary"
                onClick={update.install}
                disabled={update.check === "installing"}
                aria-live="polite"
              >
                {update.check === "installing" ? t("settings.updateDownloading") : update.check === "failed" ? t("settings.updateRetry") : t("settings.updateInstall")}
              </button>
            ) : (
              <button
                type="button"
                data-press
                className="zc-btn zc-btn-sm zc-btn-secondary"
                onClick={update.checkNow}
                disabled={update.check === "checking"}
                aria-live="polite"
              >
                {checkLabel}
              </button>
            )}
            <Switch
              id="set-auto-update"
              checked={!!update.auto}
              disabled={update.auto === null}
              onChange={update.setAuto}
            />
          </div>
        }
      />
    </div>
  </section>
  );
};
