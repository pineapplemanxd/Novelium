import { api, extractErrorMessage } from "../api/tauri";

/** Prompts for local ebook files, imports each for offline reading, and refreshes the caller. */
export async function importBookFilesFromPicker(
  onNotify: (msg: string, isError?: boolean) => void,
  onReload: () => Promise<void>,
  setImporting: (busy: boolean) => void,
  targetLabel: string,
): Promise<void> {
  try {
    const files = await api.pickBookFiles();
    if (files.length === 0) return;
    setImporting(true);
    let importedCount = 0;
    for (const f of files) {
      await api.importLocalBook(f);
      importedCount++;
    }
    onNotify(
      `Imported ${importedCount} book${importedCount === 1 ? "" : "s"} into ${targetLabel}`,
    );
    await onReload();
  } catch (e) {
    onNotify(extractErrorMessage(e), true);
  } finally {
    setImporting(false);
  }
}
