export class AdminPage {
  greeting = 'Welcome, Admin';

  async assertGreeting() {
    return this.greeting;
  }

  async unusedAdminOnly() {
    return 'unused';
  }
}
