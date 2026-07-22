import { useEffect, useState } from "react";
import { Download } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Alert, AlertDescription } from "./ui/alert";
import { Button } from "./ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./ui/dialog";
import { useI18n } from "../lib/i18n";
import type { UpdateCheckResult } from "../types";

interface UpdateAvailableDialogProps {
  update: UpdateCheckResult | null;
  onDismiss: () => void;
}

export function UpdateAvailableDialog({
  update,
  onDismiss,
}: UpdateAvailableDialogProps) {
  const { t } = useI18n();
  const [opening, setOpening] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);

  useEffect(() => {
    setOpenError(null);
  }, [update]);

  if (!update) return null;

  async function handleDownload() {
    if (!update) return;

    setOpening(true);
    setOpenError(null);
    try {
      await openUrl(update.releaseUrl);
      onDismiss();
    } catch (err) {
      setOpenError(t("about.openFailed", { error: String(err) }));
    } finally {
      setOpening(false);
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !opening) onDismiss();
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {t("update.dialogTitle", { version: update.latestVersion })}
          </DialogTitle>
          <DialogDescription>
            {t("update.dialogDescription", {
              currentVersion: update.currentVersion,
            })}
          </DialogDescription>
        </DialogHeader>

        {openError && (
          <Alert variant="destructive">
            <AlertDescription>{openError}</AlertDescription>
          </Alert>
        )}

        <DialogFooter>
          <DialogClose
            render={
              <Button type="button" variant="secondary" disabled={opening} />
            }
          >
            {t("update.later")}
          </DialogClose>
          <Button
            type="button"
            variant="primary"
            disabled={opening}
            onClick={() => void handleDownload()}
          >
            <Download data-icon="inline-start" />
            {opening ? t("update.opening") : t("update.download")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
