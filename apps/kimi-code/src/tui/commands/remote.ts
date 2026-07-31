import QRCode from 'qrcode';

import type { SlashCommandHost } from './dispatch';

export async function handleRemoteCommand(host: SlashCommandHost, args: string): Promise<void> {
  switch (args.trim().toLowerCase()) {
    case 'start': {
      const remote = await host.startRemoteAccess();
      host.showNotice(
        remote.started ? 'Remote access started' : 'Remote access is already running',
        `Runtime: ${remote.runtimeId}\n\nPaired devices connect automatically. Use /remote pair to add a device.`,
      );
      return;
    }
    case 'pair': {
      const remote = await host.pairRemoteAccess();
      const qr = await QRCode.toString(remote.pairingUri, { type: 'terminal', small: true });
      host.showNotice(
        'Pair a mobile device',
        `Runtime: ${remote.runtimeId}\n\n${qr}\n${remote.pairingUri}`,
      );
      return;
    }
    case 'stop':
      host.showNotice(
        (await host.stopRemoteAccess())
          ? 'Remote access stopped'
          : 'Remote access is already stopped',
      );
      return;
    default:
      host.showError('Usage: /remote <start|pair|stop>');
  }
}
