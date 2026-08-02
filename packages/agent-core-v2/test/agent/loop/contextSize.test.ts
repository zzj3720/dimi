import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { IConfigService } from "#/app/config/config";
import { IEventBus } from "#/app/event/eventBus";
import { DEFAULT_AGENT_PROFILE_NAME } from "#/app/agentProfileCatalog/agentProfileCatalog";
import { LOOP_CONTROL_SECTION } from "#/agent/loop/configSection";
import {
  CONTEXT_SIZE_FLOOR_TOKENS,
  contextSizePercentOptions,
  scaleContextTokens,
  scaleModelCapabilityContext,
} from "#/agent/loop/contextSize";
import { IAgentProfileService } from "#/agent/profile/profile";
import { UNKNOWN_CAPABILITY, type ModelCapability } from "#/llmProtocol/capability";

import { createTestAgent, type TestAgentContext } from "../../harness";

describe("context size helpers", () => {
  describe("scaleContextTokens", () => {
    it("passes through when percent is unset or at 100", () => {
      expect(scaleContextTokens(1_000_000, undefined)).toBe(1_000_000);
      expect(scaleContextTokens(1_000_000, 100)).toBe(1_000_000);
      expect(scaleContextTokens(0, 50)).toBe(0);
    });

    it("scales a large window by the percentage", () => {
      expect(scaleContextTokens(1_000_000, 50)).toBe(500_000);
      expect(scaleContextTokens(1_000_000, 90)).toBe(900_000);
    });

    it("never scales below the 200k floor while the window is at or above it", () => {
      expect(scaleContextTokens(1_000_000, 20)).toBe(200_000);
      // Hand-edited configs below the offered range still respect the floor.
      expect(scaleContextTokens(1_000_000, 10)).toBe(200_000);
      expect(scaleContextTokens(300_000, 60)).toBe(200_000);
      expect(scaleContextTokens(220_000, 95)).toBe(209_000);
    });

    it("leaves windows below the floor unchanged (not adjustable)", () => {
      expect(scaleContextTokens(150_000, 50)).toBe(150_000);
      expect(scaleContextTokens(199_999, 5)).toBe(199_999);
    });
  });

  describe("contextSizePercentOptions", () => {
    it("offers 100% down to the step that keeps at least 200k", () => {
      expect(contextSizePercentOptions(1_000_000)).toEqual([
        100, 95, 90, 85, 80, 75, 70, 65, 60, 55, 50, 45, 40, 35, 30, 25, 20,
      ]);
      expect(contextSizePercentOptions(220_000)).toEqual([100, 95]);
    });

    it("returns an empty list for windows already below the floor", () => {
      expect(contextSizePercentOptions(150_000)).toEqual([]);
      expect(contextSizePercentOptions(200_000)).toEqual([100]);
      expect(contextSizePercentOptions(0)).toEqual([]);
    });
  });

  describe("scaleModelCapabilityContext", () => {
    const capability: ModelCapability = {
      image_in: false,
      video_in: false,
      audio_in: false,
      thinking: true,
      tool_use: true,
      max_context_tokens: 1_000_000,
    };

    it("returns the same object when percent is unset", () => {
      expect(scaleModelCapabilityContext(capability, undefined)).toBe(capability);
      expect(scaleModelCapabilityContext(UNKNOWN_CAPABILITY, undefined)).toBe(
        UNKNOWN_CAPABILITY,
      );
    });

    it("scales both token limits and preserves modality flags", () => {
      const scaled = scaleModelCapabilityContext(capability, 80);
      expect(scaled).not.toBe(capability);
      expect(scaled.max_context_tokens).toBe(800_000);
      expect(scaled.image_in).toBe(false);
      expect(scaled.thinking).toBe(true);
    });

    it("scales max_input_tokens when present", () => {
      const withInput = { ...capability, max_input_tokens: 900_000 };
      expect(scaleModelCapabilityContext(withInput, 80)).toMatchObject({
        max_context_tokens: 800_000,
        max_input_tokens: 720_000,
      });
    });

    it("returns the same object when nothing changes (window below floor)", () => {
      const small = { ...capability, max_context_tokens: 150_000 };
      expect(scaleModelCapabilityContext(small, 50)).toBe(small);
    });
  });
});

describe("Agent profile context-size scaling", () => {
  let ctx: TestAgentContext;

  beforeEach(() => {
    ctx = createTestAgent({
      initialConfig: {
        loopControl: { contextSizePercent: 50 },
      },
    });
  });

  afterEach(async () => {
    try {
      await ctx.expectResumeMatches();
    } finally {
      await ctx.dispose();
    }
  });

  it("scales the resolved model context by the configured percentage", async () => {
    const profile = ctx.get(IAgentProfileService);
    await profile.bind({ profile: DEFAULT_AGENT_PROFILE_NAME, model: "mock-model" });

    const resolved = profile.resolveModelContext();
    expect(resolved.modelCapabilities.max_context_tokens).toBe(500_000);
    expect(profile.getModelCapabilities().max_context_tokens).toBe(500_000);
    expect(profile.data().modelCapabilities.max_context_tokens).toBe(500_000);
  });

  it("applies config changes immediately and republishes the status", async () => {
    const profile = ctx.get(IAgentProfileService);
    const config = ctx.get(IConfigService);
    await profile.bind({ profile: DEFAULT_AGENT_PROFILE_NAME, model: "mock-model" });

    const events: number[] = [];
    const sub = ctx.get(IEventBus).subscribe((event) => {
      if (event.type === "agent.status.updated" && event.maxContextTokens !== undefined) {
        events.push(event.maxContextTokens);
      }
    });
    try {
      await config.set(LOOP_CONTROL_SECTION, { contextSizePercent: 80 });
      expect(profile.resolveModelContext().modelCapabilities.max_context_tokens).toBe(800_000);
      expect(events).toContain(800_000);

      await config.set(LOOP_CONTROL_SECTION, { contextSizePercent: 5 });
      // 5% of 1M = 50k, floored to the 200k minimum.
      expect(profile.resolveModelContext().modelCapabilities.max_context_tokens).toBe(
        CONTEXT_SIZE_FLOOR_TOKENS,
      );
    } finally {
      sub.dispose();
    }
  });

  it("leaves a below-floor model unchanged even with a percentage set", async () => {
    const smallCtx = createTestAgent({
      initialConfig: {
        loopControl: { contextSizePercent: 50 },
        models: {
          "test-provider/small-model": {
            provider: "test-provider",
            model: "small-model",
            maxContextSize: 150_000,
          },
        },
      },
    });
    try {
      const profile = smallCtx.get(IAgentProfileService);
      await profile.bind({ profile: DEFAULT_AGENT_PROFILE_NAME, model: "test-provider/small-model" });

      // 50% would be 75k, but the floor is capped at the model's own window:
      // a model below 200k is not adjustable.
      expect(profile.getModelCapabilities().max_context_tokens).toBe(150_000);
      expect(profile.resolveModelContext().modelCapabilities.max_context_tokens).toBe(150_000);
      expect(profile.data().modelCapabilities.max_context_tokens).toBe(150_000);
    } finally {
      await smallCtx.dispose();
    }
  });
});
