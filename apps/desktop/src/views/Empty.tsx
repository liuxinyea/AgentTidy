// Empty-state panel shown when no supported agent installation was detected
// on this machine. Matches the CLI's "No supported agent installations
// detected." text path in `apps/cli/src/main.rs:147`.

import { useI18n } from "../i18n";

export default function Empty() {
  const { t } = useI18n();
  return (
    <div className="rounded-lg border border-dashed border-neutral-300 bg-white p-10 text-center">
      <h2 className="text-lg font-medium text-neutral-700">
        {t("empty.title")}
      </h2>
      <p className="mt-2 text-sm text-neutral-500">{t("empty.body")}</p>
    </div>
  );
}