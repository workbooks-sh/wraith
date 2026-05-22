import { BreakpointString, Status, Color, Direction } from './types';

// Mapped type: all BreakpointString members are used as keys
type BreakpointValues = { [K in BreakpointString]?: number };

// Qualified name in type position: Status.Active is used
type ActiveOnly = Status.Active;

// Record utility type: all Color members are used as keys
type ColorMap = Record<Color, string>;

// keyof typeof in mapped type: all Direction members are used as keys
type DirectionLabels = { [K in keyof typeof Direction]: string };

const breakpoints: BreakpointValues = {
    xs: 0,
    sm: 576,
};

// Runtime access — only Status.Inactive directly accessed
const s = Status.Inactive;

const colors: ColorMap = { red: '#f00', green: '#0f0', blue: '#00f' };
const dirs: DirectionLabels = { Up: 'up', Down: 'down', Left: 'left', Right: 'right' };

console.log(breakpoints, s, colors, dirs);
