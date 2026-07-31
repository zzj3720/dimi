import { MAX_IMAGE_EDGE_PX, READ_IMAGE_BYTE_BUDGET } from '@moonshot-ai/agent-core-v2';

export interface ImageLimitsConfig {
  readonly maxEdgePx?: number;
  readonly readByteBudget?: number;
}

export class ImageLimits {
  constructor(
    private readonly env: Readonly<Record<string, string | undefined>> = process.env,
    private config?: ImageLimitsConfig,
  ) {}

  setConfig(config: ImageLimitsConfig | undefined): void {
    this.config = config;
  }

  maxEdgePx(): number {
    return positiveInt(this.env['KIMI_IMAGE_MAX_EDGE_PX']) ?? this.config?.maxEdgePx ?? MAX_IMAGE_EDGE_PX;
  }

  readByteBudget(): number {
    return (
      positiveInt(this.env['KIMI_IMAGE_READ_BYTE_BUDGET']) ??
      this.config?.readByteBudget ??
      READ_IMAGE_BYTE_BUDGET
    );
  }
}

function positiveInt(value: string | undefined): number | undefined {
  if (value === undefined || !/^\d+$/.test(value.trim())) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : undefined;
}
