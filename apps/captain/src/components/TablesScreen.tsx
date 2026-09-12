import { useQuery } from "@tanstack/react-query";
import { fetchTables, type CaptainTable } from "../lib/api";
import { Empty, Waiting, errorText } from "./Waiting";

interface Props {
  token: string;
  onSelectTable: (table: CaptainTable) => void;
}

/**
 * Tables screen. Free and occupied must be distinguishable at a glance across
 * a room (docs/captain-api.md) — colour and a text tag, not colour alone.
 */
export function TablesScreen({ token, onSelectTable }: Props) {
  const query = useQuery({
    queryKey: ["captain", "tables"],
    queryFn: () => fetchTables(token),
  });

  if (query.isLoading) return <Waiting label="Loading tables…" />;
  if (query.isError) {
    return <p className="screen error">Could not load tables. {errorText(query.error)}</p>;
  }

  const tables = query.data ?? [];

  return (
    <div className="screen">
      <h2>Tables</h2>
      {tables.length === 0 && (
        <Empty
          title="No tables are set up for this outlet."
          detail="Tables come from the till's configuration. Ask the till to add them, then pull to refresh."
        />
      )}
      <div className="table-grid">
        {tables.map((t) => {
          const occupied = t.open_session_id !== null;
          return (
            <button
              key={t.id}
              type="button"
              className={`table-tile ${occupied ? "occupied" : "free"}`}
              onClick={() => onSelectTable(t)}
            >
              <span>{t.name}</span>
              <span className="seats">{t.seats} seats</span>
              <span>{occupied ? "Occupied" : "Free"}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
