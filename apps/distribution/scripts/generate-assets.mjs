import QRCode from "qrcode";
import { fileURLToPath } from "node:url";

await QRCode.toFile(fileURLToPath(new URL("../public/install-qr.svg", import.meta.url)), "https://install.k.test.3720.org", {
  type: "svg",
  color: { dark: "#17191C", light: "#FFFFFF" },
  errorCorrectionLevel: "M",
  margin: 1,
  width: 224,
});
