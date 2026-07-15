import { OrbitWidget } from '../features/orbit/OrbitWidget';
import { AppShell } from './AppShell';

export default function StandardApp() {
  return (
    <AppShell
      renderWidget={(context) => <OrbitWidget {...context} />}
      variant="standard"
    />
  );
}
