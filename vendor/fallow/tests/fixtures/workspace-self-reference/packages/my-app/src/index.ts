import { Button } from '@repro/ui-kit/button';
import { Modal } from '@repro/ui-kit/modal';

const btn = new Button('Click me');
console.log(btn.render());

const modal = new Modal('Settings');
console.log(modal.open());
