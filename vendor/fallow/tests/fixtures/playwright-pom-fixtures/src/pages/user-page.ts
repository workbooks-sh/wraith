export class UserPage {
  greeting = 'Welcome, User';

  async assertGreeting() {
    return this.greeting;
  }

  async unusedUserOnly() {
    return 'unused';
  }
}
