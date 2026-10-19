import { useAppStore } from "../../store";

export function ProfileSwitcher() {
  const { profiles, activeProfile } = useAppStore();
  if (profiles.length === 0) return null;
  return (
    <div className="flex gap-1 rounded-xl bg-black/30 p-1" role="tablist" aria-label="Perfil">
      {profiles.map((profile) => {
        const active = profile.id === activeProfile;
        return (
          <button
            key={profile.id}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() =>
              useAppStore.setState({ activeProfile: profile.id, activeProject: null, projects: [] })
            }
            className="flex-1 rounded-lg px-3 py-1.5 text-sm font-medium transition-colors"
            style={
              active
                ? { background: `${profile.color}33`, color: profile.color }
                : { color: "rgb(148 163 184)" }
            }
          >
            {profile.label}
          </button>
        );
      })}
    </div>
  );
}
