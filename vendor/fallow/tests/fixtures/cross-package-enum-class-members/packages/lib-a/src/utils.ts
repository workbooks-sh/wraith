export class StringUtils {
  static toUpper(s: string): string {
    return s.toUpperCase();
  }
  static toLower(s: string): string {
    return s.toLowerCase();
  }
  static reverse(s: string): string {
    return s.split('').reverse().join('');
  }
}
