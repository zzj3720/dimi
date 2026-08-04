import { css } from '@emotion/css';
import { colors } from './styles/theme';

export const shell = css({
  display: 'flex',
  height: '100%',
});

// Codex: main content surface is #181818 over body #141414
export const mainCol = css({
  display: 'flex',
  flexDirection: 'column',
  flex: 1,
  minWidth: 0,
  background: colors.surface,
});
