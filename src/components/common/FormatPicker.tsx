import React from "react";
import { useTranslation } from "react-i18next";
import { filterOutputFormats } from "../../application/formatCatalog.mjs";

interface FormatPickerProps {
  options: string[];
  value: string;
  onChange: (format: string) => void;
  disabled: boolean;
  label: string;
}

/** Compact searchable picker; 40+ advertised formats do not expand the left rail. */
export const FormatPicker: React.FC<FormatPickerProps> = ({
  options, value, onChange, disabled, label
}) => {
  const { t } = useTranslation();
  const [search, setSearch] = React.useState("");

  // Fixed alphabetical order: a chip never moves after it is picked.
  const matching = [...filterOutputFormats(options, search)].sort((a, b) => a.localeCompare(b));

  const renderOption = (id: string) => (
    <button
      key={id}
      type="button"
      disabled={disabled}
      onClick={() => onChange(id)}
      aria-pressed={value === id}
      aria-label={t(value === id ? "picker.optionSelected" : "picker.optionLabel", { format: id.toUpperCase() })}
      className="zc-chip"
    >
      {id.toUpperCase()}
    </button>
  );

  return (
    <div role="group" aria-label={label} className="space-y-2">
      {options.length > 12 && (
        <input
          type="search"
          autoComplete="off"
          spellCheck={false}
          value={search}
          disabled={disabled}
          onChange={e => setSearch(e.target.value)}
          placeholder={t("picker.searchPlaceholder")}
          aria-label={t("picker.searchLabel")}
          className="zc-field"
        />
      )}
      {options.length ? (
        <div className="space-y-2">
          {matching.length > 0 && (
            <div className="grid grid-cols-4 gap-1">{matching.map(renderOption)}</div>
          )}
          {!matching.length && <p className="text-[12px] text-text-tertiary">{t("picker.noMatch")}</p>}
        </div>
      ) : (
        <p className="text-[12px] text-warning">{t("picker.noRoute")}</p>
      )}
    </div>
  );
};
