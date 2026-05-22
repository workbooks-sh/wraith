import { Component } from '@angular/core';
import { DataService } from './data.service';

@Component({
  selector: 'app-data-view',
  templateUrl: './data-view.component.html',
})
export class DataViewComponent {
  constructor(public dataService: DataService) {}
}
