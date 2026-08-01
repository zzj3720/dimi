/**
 * `loop` domain (L4) — Agent turn continuation aspect.
 *
 * Drives another step after tool results and after every tool-free response.
 * Tool-free responses receive the completion reminder; only a control path
 * that stops the turn can suppress continuation. Bound at Agent scope.
 */

import { Disposable } from '#/_base/di/lifecycle';
import { LifecycleScope, ScopeActivation, registerScopedService } from '#/_base/di/scope';
import { COMPLETION_REVIEW_MIN_STEPS, COMPLETION_REVIEW_REMINDER } from '#/agent/completion/completion';
import { IAgentProfileService } from '#/agent/profile/profile';
import { IAgentSystemReminderService } from '#/agent/systemReminder/systemReminder';

import { IAgentLoopContinuationService } from './loopContinuation';
import { IAgentLoopService } from './loop';
import { ContinuationStepRequest } from './stepRequest';

export class AgentLoopContinuationService
  extends Disposable
  implements IAgentLoopContinuationService
{
  declare readonly _serviceBrand: undefined;

  constructor(
    @IAgentLoopService loop: IAgentLoopService,
    @IAgentProfileService profile: IAgentProfileService,
    @IAgentSystemReminderService reminders: IAgentSystemReminderService,
  ) {
    super();
    this._register(
      loop.hooks.onDidFinishStep.register('loop-continuation', async (ctx, next) => {
        await next();
        if (ctx.stopTurn || ctx.finishReason === 'filtered') return;
        if (ctx.toolCalls.length === 0) {
          if (!profile.isRunnable()) return;
          // Short turns may end with a plain text reply: the completion
          // review only kicks in after enough steps, so quick answers finish
          // naturally without the AllDone ceremony.
          if (ctx.step < COMPLETION_REVIEW_MIN_STEPS) return;
          reminders.appendSystemReminder(COMPLETION_REVIEW_REMINDER, {
            kind: 'system_trigger',
            name: 'completion_review',
          });
          loop.enqueue(new ContinuationStepRequest());
          return;
        }
        if (ctx.finishReason !== 'tool_calls') return;
        loop.enqueue(new ContinuationStepRequest());
      }),
    );
  }
}

registerScopedService(
  LifecycleScope.Agent,
  IAgentLoopContinuationService,
  AgentLoopContinuationService,
  ScopeActivation.OnScopeCreated,
  'loop',
);
