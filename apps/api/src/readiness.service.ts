import { Injectable, type BeforeApplicationShutdown } from "@nestjs/common";

@Injectable()
export class ReadinessService implements BeforeApplicationShutdown {
  private acceptingTraffic = true;

  beforeApplicationShutdown(): void {
    this.markDraining();
  }

  markDraining(): void {
    this.acceptingTraffic = false;
  }

  isAccepting(): boolean {
    return this.acceptingTraffic;
  }
}
