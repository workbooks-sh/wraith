export class Box {
    count = 0;

    bump(): void {
        this.count += 1;
    }

    get current(): number {
        return this.count;
    }

    set current(value: number) {
        this.count = value;
    }

    unused(): void {}
}
