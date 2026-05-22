// Expression body member access: import('./x').then(m => m.foo)
const result1 = import('./lib').then(m => m.foo);

// Destructured parameter: import('./x').then(({ bar, baz }) => ...)
import('./lib').then(({ bar, baz }) => {
  console.log(bar, baz);
});

// Angular routing pattern with .then(m => m.Component)
const routes = [
  {
    path: 'dashboard',
    loadComponent: () => import('./dashboard.component').then(m => m.DashboardComponent),
  },
  {
    path: 'settings',
    loadComponent: () => import('./settings.component').then(m => m.SettingsComponent),
  },
];

export { result1, routes };
