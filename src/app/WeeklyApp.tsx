import { WeeklyOrbitWidget } from '../features/orbit/WeeklyOrbitWidget';
import { AppShell } from './AppShell';

export default function WeeklyApp() {
  return (
    <AppShell
      renderWidget={(context) => <WeeklyOrbitWidget {...context} />}
      variant="weekly"
    />
  );
}
