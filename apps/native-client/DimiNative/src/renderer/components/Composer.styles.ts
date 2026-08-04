import { css } from '@emotion/css';
import { colors, font, size, radius } from '../styles/theme';

// ---- composer shell ----
// Codex root: form.relative.flex.flex-col (data-thread-find-composer) — layout
// only, no background/border; the visual capsule is the nested surface layer.
export const composer = css({
  position: 'relative',
  flexShrink: 0,
  padding: '0 0 16px', // sticky wrapper pb-4 (16px)
});

// Codex wrapper between form and surface: div.relative.flex.w-full.flex-col.gap-2.
export const composerWrapper = css({
  position: 'relative',
  display: 'flex',
  width: '100%',
  flexDirection: 'column',
  gap: 8,
});

// Codex bds ComposerSurface — the visual capsule (thread composer defaults:
// multiline surface, bg 90% + blur(16px), overflow-y-auto, elevation shadow).
// Height follows content (empty = 98px: 14 attachments + 76 footer + 8 mb-2);
// there is NO fixed min-height and NO border / focus-within rule (the only
// focus feedback is the white caret in the editor).
// Width = codex two-layer formula `min(768, 主区) − 32` (reverse-engineered:
// w-full + max-w-(--thread-content-max-width:48rem) + px-toolbar 16×2 →
// min(736, calc(100% − 32px))); keeps a 16px gutter in narrow windows instead
// of the old fixed 736px that squeezed the capsule to a 3px edge gap.
export const capsule = css({
  width: 'min(736px, calc(100% - 32px))',
  margin: '0 auto',
  background: colors.composerBg,
  backdropFilter: 'blur(16px)',
  WebkitBackdropFilter: 'blur(16px)',
  borderRadius: size.composerRadius,
  display: 'flex',
  flexDirection: 'column',
  boxSizing: 'border-box',
  overflowY: 'auto', // multiline surfaceOverflow=auto; single-line overrides to visible
  // Hairline ring: 0.5px box-shadow spread (codex elevation-prominent); no border.
  boxShadow:
    'rgba(255, 255, 255, 0.157) 0 0 0 0.5px, rgba(0, 0, 0, 0.04) 0 3px 7.5px 0, rgba(0, 0, 0, 0.05) 0 0 20px 0',
});

// Codex single-line surface: overflow-visible (no scroll container) + codex
// single-line capsule radius 22px (`--radius-token-composer-single-line` =
// .25rem × 5.5; design 04 §7.2). Overrides the multiline capsule's 25px
// (radius-3xl): this class's rule is inserted after `capsule` in the emotion
// stylesheet, so it wins at equal specificity when both classes are applied.
export const surfaceSingle = css({
  overflow: 'visible',
  overflowY: 'visible',
  borderRadius: 22,
});

// Codex Pds Body: div.relative.z-10.flex.min-h-0.flex-1.flex-col.
export const surfaceBody = css({
  position: 'relative',
  zIndex: 10,
  display: 'flex',
  minHeight: 0,
  flex: 1,
  flexDirection: 'column',
});

// Codex xds Attachments slot: _attachmentsDefault_1xj1z_2 — 8px inset + 6px
// bottom = 14px empty-state height. dimi has no attachment UI, the slot stays
// empty (the 附件 button still reports 暂未实现).
export const attachments = css({
  padding: '8px 8px 6px',
});

// Codex bvs hidden text-measure span (auto-single-line fit test):
// pointer-events-none invisible absolute h-0 w-max max-w-none overflow-hidden
// whitespace-pre text-size-chat — same font as the editor input.
export const measure = css({
  position: 'absolute',
  visibility: 'hidden',
  pointerEvents: 'none',
  height: 0,
  width: 'max-content',
  maxWidth: 'none',
  overflow: 'hidden',
  whiteSpace: 'pre',
  fontSize: 14,
  fontWeight: 445,
});

// Codex Tds Footer (multiline): grid grid-cols-[minmax(0,auto)_auto_minmax(0,1fr)]
// items-center gap-x-[5px] select-none mb-2 px-2. _footer_1xj1z_2 is the
// container-query anchor: hides the footer label ≤440px (electron ≤475px).
export const footer = css({
  display: 'grid',
  gridTemplateColumns: 'minmax(0, auto) auto minmax(0, 1fr)',
  alignItems: 'center',
  columnGap: 5,
  padding: '0 8px',
  marginBottom: 8, // mb-2; the 98px empty height = 14 + 76 + 8
  minHeight: 76, // 48 (input row) + 28 (button row) — codex height is content-driven
  userSelect: 'none',
  containerType: 'inline-size',
});

// Codex single-line footer: grid-cols-[auto_minmax(0,1fr)_auto] gap-2 px-2 py-1
// (no mb-2, no fixed min-height).
export const footerSingle = css({
  gridTemplateColumns: 'auto minmax(0, 1fr) auto',
  columnGap: 8,
  rowGap: 8,
  padding: '4px 8px',
  marginBottom: 0,
  minHeight: 0,
});

// Multiline input row: col-span-full row-start-1 with -mx-2 cancelling the
// footer px-2 so the input spans the full capsule width (codex AdaptiveFooter
// placement; the middle grid column stays an empty placeholder for single-line).
export const inputRow = css({
  gridColumn: '1 / -1',
  gridRow: 1,
  minWidth: 0,
  margin: '0 -8px',
});

export const inputRowSingle = css({
  gridColumn: 2,
  gridRow: 1,
  margin: 0,
});

// Codex Sds Input (multiline): mb-1 flex-grow overflow-y-auto px-3; the
// editor's own min-height (2.75rem = 44px) drives the row height.
export const inputWrap = css({
  flexGrow: 1,
  overflowY: 'auto',
  padding: '0 12px', // Codex px-3: text column starts 12px from the capsule edge
  minHeight: 44, // Codex ProseMirror min-height 44
  marginBottom: 4, // Codex mb-1: input row 48 = 44 content + 4 below
  // Top-aligned (Codex): no align-items, so the editable stretches to the
  // 44px min-height and the text starts at the top of the row.
  display: 'flex',
});

// Codex single-line editor host: h-9 (36px) flex items-center overflow-hidden,
// no px-3 / mb-1 (the footer px-2 + gap-2 provide the spacing).
export const inputWrapSingle = css({
  height: 36,
  minHeight: 0,
  marginBottom: 0,
  padding: 0,
  alignItems: 'center',
  overflow: 'hidden',
  overflowY: 'hidden',
});

export const composerLeft = css({ gridColumn: 1, gridRow: 2, minWidth: 0 });
export const composerLeftSingle = css({ gridColumn: 1, gridRow: 1 });

// Codex FooterControls: multiline = flex min-w-0 items-center justify-end w-full.
export const composerRight = css({
  gridColumn: 3,
  gridRow: 2,
  minWidth: 0,
  width: '100%',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'flex-end',
});

// Codex single-line FooterControls: shrink-0 gap-2 (no w-full).
export const composerRightSingle = css({
  gridColumn: 3,
  gridRow: 1,
  width: 'auto',
  flexShrink: 0,
  gap: 8,
});

// Codex FooterExpandingControls: flex min-w-0 flex-1 justify-end — the model
// pill's elastic placeholder; the pill truncates at max-w-48 on narrow windows.
export const composerExpanding = css({
  display: 'flex',
  minWidth: 0,
  flex: 1,
  justifyContent: 'flex-end',
});

// Codex FooterActions: flex shrink-0 items-center gap-2 (听写 ↔ 发送 8px;
// pill ↔ 听写 0px in multiline comes from the expanding div's gap-less adjacency).
export const composerActions = css({
  display: 'flex',
  flexShrink: 0,
  alignItems: 'center',
  gap: 8,
});

export const composerBtn = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 4,
  width: size.composerBtn,
  height: size.composerBtn,
  padding: 0,
  border: '1px solid transparent', // Codex: 1px but transparent
  borderRadius: radius.pill,
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: font.sm,
  lineHeight: '18px',
  cursor: 'default', // Codex cursor-interaction resolves to `default` on macOS
  flexShrink: 0,
  // Codex: hover only changes bg (--vscode-list-hoverBackground), color stays.
  '&:hover': { background: 'rgba(255, 255, 255, 0.078)' },
  '&:active': { background: 'rgba(255, 255, 255, 0.15)' }, // token-foreground @ 15%
  '&:disabled': { opacity: 0.4, cursor: 'not-allowed' },
  // Ghost icons (plus / mic) are pure white (#fff, token-text) — the button's
  // own color stays tertiary for any text content; svg fill=currentColor
  // resolves against the svg's own color.
  '& svg': { width: 16, height: 16, color: colors.text },
});

export const modelPill = css({
  display: 'flex',
  alignItems: 'center',
  gap: 4,
  padding: '0 8px',
  border: '1px solid transparent',
  borderRadius: radius.pill,
  background: 'transparent',
  color: colors.textTertiary,
  fontSize: font.sm,
  lineHeight: '18px',
  fontWeight: 445, // Codex text-sm weight 445
  cursor: 'default',
  whiteSpace: 'nowrap',
  height: size.composerBtn,
  minWidth: 0,
  maxWidth: 192, // Codex max-w-48 (12rem); truncates via the name span
  overflow: 'hidden',
  // Codex: hover/active only change bg, color stays tertiary.
  '&:hover': { background: 'rgba(255, 255, 255, 0.078)' },
  '&:active': { background: 'rgba(255, 255, 255, 0.15)' },
  '&:disabled': { opacity: 0.4, cursor: 'not-allowed' },
  '& svg': { width: 14, height: 14, flexShrink: 0 }, // chevron 14×14
});

// Model name is token-text (#fff); truncates (Codex span.truncate) and hides on
// narrow footers (Codex _footerLabel container query, electron ≤475px).
export const modelPillName = css({
  color: colors.text,
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  '@container (max-width: 475px)': { display: 'none' },
});

export const modelPillMode = css({ color: colors.textTertiary });

export const input = css({
  flex: 1,
  border: 'none',
  background: 'transparent',
  color: colors.text,
  font: 'inherit',
  fontSize: 14,
  lineHeight: '20px',
  fontWeight: 445, // Codex input weight 445
  padding: 0,
  maxHeight: '25dvh', // Codex max-h-[25dvh] (240px @ 960 viewport), then scrolls
  outline: 'none',
  overflowY: 'auto',
  display: 'block',
  width: '100%',
  whiteSpace: 'break-spaces',
  wordBreak: 'break-word',
  userSelect: 'text',
  caretColor: colors.text,
  '&:empty::before': {
    content: 'attr(data-placeholder)',
    color: colors.textTertiary, // Codex placeholder = text-tertiary rgba(255,255,255,0.498)
    opacity: 0.5, // Codex adds opacity .5 → final ≈ rgba(255,255,255,.249)
    pointerEvents: 'none',
  },
});

// Codex single-line ProseMirror: !h-5 !min-h-5 leading-5 nowrap ellipsis
// overflow-hidden (clips instead of wrapping/scroll).
export const inputSingle = css({
  height: 20,
  minHeight: 20,
  maxHeight: 'none',
  overflow: 'hidden',
  overflowY: 'hidden',
  whiteSpace: 'nowrap',
  textOverflow: 'ellipsis',
  wordBreak: 'normal',
});

export const sendBtn = css({
  flexShrink: 0,
  width: size.sendBtn,
  height: size.sendBtn,
  borderRadius: radius.pill,
  border: 'none',
  background: '#fff', // Codex bg-token-foreground
  color: colors.composerBg, // Codex arrow fill = token-dropdown-background rgb(45,45,45)
  fontSize: 15,
  lineHeight: 1,
  cursor: 'default',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  padding: 2,
  // 听写 ↔ 发送 gap 8px comes from FooterActions gap-2 (composerActions).
  transition: 'opacity 0.15s cubic-bezier(0.4, 0, 0.2, 1)',
  // Codex has NO hover background (class carries only transition-opacity).
  '&:focus-visible': { outline: '2px solid rgb(13, 13, 13)' }, // Codex outline-2 token-button-background
  '&:disabled': { opacity: 0.5, cursor: 'default' },
  '& svg': { width: 16, height: 16 },
});

export const queuedCount = css({ color: colors.textMuted, fontSize: 12 });

// Busy-state controls float ABOVE the capsule (TUI functionality preserved
// without pushing the capsule off Codex's 98px composer geometry). Codex has
// no such toolbar — dimi TUI leftover, kept for steer/queue/Cancel.
export const composerToolbar = css({
  position: 'absolute',
  bottom: 'calc(100% + 8px)',
  left: '50%',
  transform: 'translateX(-50%)',
  width: 'min(768px, calc(100vw - 32px))',
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  padding: '6px 10px',
  minHeight: 26,
  background: 'rgba(24, 24, 24, 0.92)',
  backdropFilter: 'blur(10px)',
  WebkitBackdropFilter: 'blur(10px)',
  borderRadius: 12,
  border: `1px solid ${colors.border}`,
  boxShadow: '0 8px 24px rgba(0, 0, 0, 0.45)',
  zIndex: 15,
});

export const hint = css({ color: colors.textMuted, fontSize: 12 });
export const footerRight = css({ color: colors.textMuted, fontSize: 12, whiteSpace: 'nowrap' });

// ---- busy-state buttons ----
export const btn = css({
  fontSize: font.sm,
  padding: '5px 12px',
  borderRadius: radius.lg, // 12.5px — codex button radius (was pill/9999px)
  border: '1px solid transparent',
  background: colors.hover,
  color: colors.text,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
  transition: 'background 0.12s ease',
  '&:hover': { background: colors.hoverStrong },
  '&:disabled': { opacity: 0.4, cursor: 'default' },
});

export const btnGhost = css({
  borderColor: colors.border,
  background: 'transparent',
  '&:hover': { background: colors.hover },
});

// ---- completion popup (floats ABOVE the capsule + 16px bottom margin)
export const completion = css({
  position: 'absolute',
  left: '50%',
  transform: 'translateX(-50%)',
  bottom: 128,
  width: 'min(640px, calc(100vw - 48px))',
  background: colors.surface2,
  border: `1px solid ${colors.border}`,
  borderRadius: 12,
  maxHeight: 260,
  overflowY: 'auto',
  zIndex: 50,
  boxShadow: '0 8px 32px rgba(0, 0, 0, 0.5)',
  padding: 4,
});

export const completionItem = css({
  display: 'flex',
  gap: 10,
  alignItems: 'center',
  padding: '6px 10px',
  cursor: 'pointer',
  borderRadius: 8,
  '&:hover': { background: colors.hover },
});

export const completionPointer = css({ flexShrink: 0, width: '2ch', color: colors.textMuted });
export const completionValue = css({ color: colors.text });
export const completionDesc = css({
  color: colors.textMuted,
  fontSize: font.sm,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
});

export const completionSelected = css({
  background: colors.hoverStrong,
  [`& .${completionPointer}`]: { color: colors.primary },
  [`& .${completionValue}`]: { color: colors.text },
});

// ---- Codex Work model picker panel (04-composer §5) ----
// Opens above the capsule (composerWrapper is position:relative), styled like
// the codex model picker: surface panel + hover rows with model + strength.
export const modelPicker = css({
  position: 'absolute',
  bottom: 'calc(100% + 8px)',
  right: 0,
  minWidth: 260,
  maxHeight: '40vh',
  overflowY: 'auto',
  background: colors.surface2,
  borderRadius: 14,
  border: `1px solid ${colors.border}`,
  boxShadow: '0 8px 32px rgba(0, 0, 0, 0.5)',
  padding: 4,
  zIndex: 60,
});

export const modelPickerList = css({
  display: 'flex',
  flexDirection: 'column',
  gap: 1,
});

export const modelPickerItem = css({
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 12,
  padding: '6px 10px',
  borderRadius: 8,
  border: 'none',
  background: 'transparent',
  color: colors.text,
  fontSize: font.sm,
  lineHeight: '18px',
  cursor: 'default',
  textAlign: 'left',
  '&:hover': { background: colors.hover },
});

export const modelPickerItemName = css({
  flex: 1,
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
});

export const modelPickerItemEffort = css({
  flexShrink: 0,
  color: colors.textMuted,
  fontSize: font.xs,
});

export const modelPickerItemSelected = css({
  background: colors.hoverStrong,
  [`& .${modelPickerItemEffort}`]: { color: colors.text },
});
