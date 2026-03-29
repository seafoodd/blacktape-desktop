import { open } from "@tauri-apps/plugin-dialog";

export const pickFolder = async (): Promise<string | null> => {
  const dir = await open({
    directory: true,
    multiple: false,
  });

  return typeof dir === "string" ? dir : null;
};
