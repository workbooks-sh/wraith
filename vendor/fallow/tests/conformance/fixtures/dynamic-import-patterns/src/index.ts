// Dynamic import with a string literal
const loadPage = async (name: string) => {
  const mod = await import(`./pages/${name}`);
  return mod.default;
};

// Dynamic import with a direct string
const loadHome = async () => {
  const { render } = await import('./pages/home');
  return render;
};

// Dynamic import for locale files
const loadLocale = async (lang: string) => {
  return import(`./locales/${lang}`);
};

// Static import for a directly used module
import { staticHelper } from './static-utils';

console.log(loadPage, loadHome, loadLocale, staticHelper());
