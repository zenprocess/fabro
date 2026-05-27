import { useDocumentTitle } from "../hooks/use-document-title";
import TerminalView from "../components/terminal-view";
import { ToastProvider } from "../components/toast";

export default function RunTerminal({ params }: { params: { id: string } }) {
  useDocumentTitle(`Terminal · ${params.id} · Fabro`);

  return (
    <ToastProvider>
      <div className="h-screen w-screen overflow-hidden">
        <TerminalView runId={params.id} chromeless />
      </div>
    </ToastProvider>
  );
}
