const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "accent": "#E0522D",
  "sidebarBg": "#111827",
  "surface": "#FFFFFF",
  "pageBg": "#F4F0E8",
  "density": 1
}/*EDITMODE-END*/;

const NAV_GROUPS = [
  {
    label: 'Workspace',
    items: ['Overview', 'Pipeline', 'Accounts', 'Reports'],
  },
  {
    label: 'Operations',
    items: ['Inbox', 'Automations', 'Approvals'],
  },
];

const KPI_ROWS = [
  { label: 'Qualified pipeline', value: '$2.48M', delta: '+12.4%' },
  { label: 'Active accounts', value: '184', delta: '+9' },
  { label: 'At-risk renewals', value: '7', delta: '-3' },
];

const DEALS = [
  { name: 'Northstar Labs', stage: 'Security review', owner: 'Mina', value: '$240K', risk: 'Low' },
  { name: 'Atlas Health', stage: 'Procurement', owner: 'Jon', value: '$180K', risk: 'Medium' },
  { name: 'Kite Finance', stage: 'Pilot', owner: 'Rae', value: '$96K', risk: 'Low' },
  { name: 'FieldWorks', stage: 'Discovery', owner: 'Ari', value: '$72K', risk: 'High' },
];

function _App() {
  const [active, setActive] = React.useState('Pipeline');
  const [drawerOpen, setDrawerOpen] = React.useState(false);
  const gap = 16 * TWEAK_DEFAULTS.density;

  return (
    <div
      style={{
        minHeight: '100%',
        background: TWEAK_DEFAULTS.pageBg,
        color: '#151821',
        fontFamily: 'DM Sans, system-ui, sans-serif',
        display: 'grid',
        gridTemplateColumns: '240px minmax(0, 1fr)',
      }}
    >
      <aside
        style={{
          background: TWEAK_DEFAULTS.sidebarBg,
          color: '#F9FAFB',
          padding: 18,
          display: 'flex',
          flexDirection: 'column',
          gap: 20,
          minHeight: '100vh',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div
            style={{
              width: 34,
              height: 34,
              borderRadius: 10,
              background: TWEAK_DEFAULTS.accent,
              display: 'grid',
              placeItems: 'center',
              fontWeight: 800,
            }}
          >
            C
          </div>
          <div>
            <div style={{ fontWeight: 800 }}>Console</div>
            <div style={{ fontSize: 12, opacity: 0.64 }}>Revenue ops</div>
          </div>
        </div>

        {NAV_GROUPS.map((group) => (
          <nav key={group.label} aria-label={group.label}>
            <div
              style={{
                fontSize: 11,
                textTransform: 'uppercase',
                letterSpacing: '0.12em',
                opacity: 0.48,
                margin: '0 0 8px 8px',
              }}
            >
              {group.label}
            </div>
            <div style={{ display: 'grid', gap: 4 }}>
              {group.items.map((item) => {
                const selected = active === item;
                return (
                  <button
                    key={item}
                    type="button"
                    onClick={() => setActive(item)}
                    style={{
                      minHeight: 40,
                      border: 0,
                      borderRadius: 10,
                      padding: '0 12px',
                      color: '#F9FAFB',
                      background: selected ? 'rgba(255,255,255,0.13)' : 'transparent',
                      boxShadow: selected ? `inset 3px 0 0 ${TWEAK_DEFAULTS.accent}` : 'none',
                      textAlign: 'left',
                      cursor: 'pointer',
                      font: 'inherit',
                    }}
                  >
                    {item}
                  </button>
                );
              })}
            </div>
          </nav>
        ))}

        <button
          type="button"
          onClick={() => setDrawerOpen(true)}
          style={{
            marginTop: 'auto',
            minHeight: 44,
            border: 0,
            borderRadius: 12,
            background: TWEAK_DEFAULTS.accent,
            color: '#fff',
            fontWeight: 800,
            cursor: 'pointer',
          }}
        >
          New account
        </button>
      </aside>

      <main style={{ padding: 28, minWidth: 0 }}>
        <header
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 16,
            marginBottom: 24,
          }}
        >
          <div>
            <div style={{ fontSize: 13, color: '#6B7280' }}>Workspace / {active}</div>
            <h1 style={{ margin: '4px 0 0', fontSize: 34, letterSpacing: -0.8 }}>
              Pipeline command center
            </h1>
          </div>
          <label
            style={{
              minWidth: 280,
              minHeight: 44,
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              borderRadius: 999,
              background: TWEAK_DEFAULTS.surface,
              padding: '0 16px',
              border: '1px solid rgba(17,24,39,0.1)',
            }}
          >
            <span aria-hidden="true">Search</span>
            <input
              aria-label="Search accounts"
              placeholder="Account, owner, stage"
              style={{ flex: 1, border: 0, outline: 0, background: 'transparent', font: 'inherit' }}
            />
          </label>
        </header>

        <section style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap }}>
          {KPI_ROWS.map((kpi) => (
            <article
              key={kpi.label}
              style={{
                background: TWEAK_DEFAULTS.surface,
                borderRadius: 16,
                padding: 20,
                border: '1px solid rgba(17,24,39,0.08)',
              }}
            >
              <div style={{ color: '#6B7280', fontSize: 13 }}>{kpi.label}</div>
              <div style={{ fontSize: 32, fontWeight: 800, marginTop: 10 }}>{kpi.value}</div>
              <div style={{ color: TWEAK_DEFAULTS.accent, fontSize: 13, marginTop: 6 }}>
                {kpi.delta} this quarter
              </div>
            </article>
          ))}
        </section>

        <section
          style={{
            marginTop: gap,
            display: 'grid',
            gridTemplateColumns: 'minmax(0, 1.25fr) 360px',
            gap,
          }}
        >
          <article
            style={{
              background: TWEAK_DEFAULTS.surface,
              borderRadius: 18,
              padding: 20,
              border: '1px solid rgba(17,24,39,0.08)',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 16 }}>
              <h2 style={{ margin: 0, fontSize: 18 }}>Priority accounts</h2>
              <button type="button" style={{ border: 0, background: 'transparent', color: TWEAK_DEFAULTS.accent }}>
                Export
              </button>
            </div>
            <div style={{ display: 'grid', gap: 10 }}>
              {DEALS.map((deal) => (
                <div
                  key={deal.name}
                  style={{
                    display: 'grid',
                    gridTemplateColumns: '1.2fr 1fr 0.7fr 0.7fr 0.6fr',
                    gap: 12,
                    alignItems: 'center',
                    minHeight: 52,
                    borderRadius: 12,
                    background: '#F8FAFC',
                    padding: '0 14px',
                  }}
                >
                  <strong>{deal.name}</strong>
                  <span>{deal.stage}</span>
                  <span>{deal.owner}</span>
                  <span>{deal.value}</span>
                  <span
                    style={{
                      justifySelf: 'start',
                      padding: '4px 8px',
                      borderRadius: 999,
                      background: deal.risk === 'High' ? '#FEE2E2' : '#E0F2FE',
                      color: deal.risk === 'High' ? '#991B1B' : '#075985',
                      fontSize: 12,
                    }}
                  >
                    {deal.risk}
                  </span>
                </div>
              ))}
            </div>
          </article>

          <aside
            style={{
              background: '#151821',
              color: '#F9FAFB',
              borderRadius: 18,
              padding: 20,
              display: 'grid',
              alignContent: 'space-between',
              minHeight: 330,
            }}
          >
            <div>
              <h2 style={{ margin: 0, fontSize: 20 }}>Review queue</h2>
              <p style={{ color: '#CBD5E1', lineHeight: 1.5 }}>
                Three approvals need owner follow-up before the end of week.
              </p>
            </div>
            <button
              type="button"
              onClick={() => setDrawerOpen(true)}
              style={{
                minHeight: 44,
                border: 0,
                borderRadius: 12,
                background: '#fff',
                color: '#151821',
                fontWeight: 800,
                cursor: 'pointer',
              }}
            >
              Open approvals
            </button>
          </aside>
        </section>
      </main>

      {drawerOpen && (
        <div
          role="dialog"
          aria-label="New account"
          onClick={() => setDrawerOpen(false)}
          style={{
            position: 'fixed',
            inset: 0,
            background: 'rgba(17,24,39,0.42)',
            display: 'grid',
            placeItems: 'end',
          }}
        >
          <div
            onClick={(event) => event.stopPropagation()}
            style={{
              width: 420,
              maxWidth: '100%',
              height: '100%',
              background: TWEAK_DEFAULTS.surface,
              padding: 24,
              boxShadow: '-24px 0 64px rgba(17,24,39,0.24)',
            }}
          >
            <h2 style={{ marginTop: 0 }}>Create account</h2>
            <p style={{ color: '#6B7280' }}>Capture a qualified opportunity and assign an owner.</p>
            <button
              type="button"
              onClick={() => setDrawerOpen(false)}
              style={{
                minHeight: 44,
                border: 0,
                borderRadius: 12,
                background: TWEAK_DEFAULTS.accent,
                color: '#fff',
                fontWeight: 800,
                cursor: 'pointer',
              }}
            >
              Save draft
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
