import "i18next";
import type { it } from "../locales/it";

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: { translation: typeof it };
  }
}
