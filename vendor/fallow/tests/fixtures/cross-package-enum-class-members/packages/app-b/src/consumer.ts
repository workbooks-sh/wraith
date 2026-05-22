import { StatusCode, Direction, StringUtils } from '@repro/lib-a';

export function getLabel(status: StatusCode): string {
  switch (status) {
    case StatusCode.Active:
      return 'Active';
    case StatusCode.Inactive:
      return 'Inactive';
    case StatusCode.Pending:
      return 'Pending';
    default:
      return 'Unknown';
  }
}

export function isHorizontal(dir: Direction): boolean {
  return dir === Direction.East || dir === Direction.West;
}

export function shout(s: string): string {
  return StringUtils.toUpper(s);
}
