# codex-usage-tray

Ícone na bandeja do Windows (perto do relógio) que mostra o uso das janelas de cota do OpenAI Codex, quanto tempo falta para o reset e, quando disponíveis, créditos e resets acumulados — sem precisar abrir a página de uso toda hora.

> Projeto irmão do [claude-usage-tray](https://github.com/gabrielsouto/claude-usage-tray), mas usando a interface suportada do **Codex app-server** em vez de ler/copiar tokens de autenticação.

## O que ele faz

- Ícone em forma de anel que se preenche conforme o uso: verde → amarelo → vermelho (limiares configuráveis). Cinza indica erro ao consultar o Codex.
- Tooltip com cada janela que o Codex realmente retornar, por exemplo:
  - `5h: 34% usado · reseta 20:00 (2h13)`
  - `7d: 12% usado · reseta 13/08 09:00 (4d14h)`
- Não presume que `primary` seja sempre 5h. A identificação é feita por `windowDurationMins`, porque o backend pode retornar apenas a janela semanal em alguns cenários.
- Mostra saldo de créditos quando o backend o fornece.
- Mostra quantidade de rate-limit resets disponíveis quando o backend o fornece.
- Notificações do Windows ao cruzar limiares de uso e quando uma nova janela começa.
- Menu (clique direito): atualizar agora, abrir página de uso do Codex, abrir configurações, sair.
- Português ou inglês, com detecção automática do idioma do Windows.

## Como ele obtém os dados

O app inicia uma instância curta de:

```text
codex app-server --stdio
```

faz o handshake JSON-RPC oficial do app-server e chama:

```text
account/rateLimits/read
```

A resposta do Codex contém, entre outros campos, `usedPercent`, `windowDurationMins`, `resetsAt`, `credits`, `planType` e os rate-limit reset credits disponíveis.

### Por que usar app-server?

O `codex-usage-tray` **não lê `~/.codex/auth.json`, não copia access tokens e não implementa OAuth/refresh token**. Autenticação, armazenamento seguro e renovação de credenciais continuam sendo responsabilidade do próprio Codex instalado na máquina.

Isso também evita depender diretamente de endpoints privados como `/backend-api/wham/usage`.

## Requisitos

- Windows 10/11.
- Codex CLI instalado e disponível como `codex` no `PATH`.
- Codex logado com uma conta ChatGPT para a qual `account/rateLimits/read` retorne limites de uso.
- Para compilar: Rust 1.75+.

Teste primeiro no terminal:

```powershell
codex --version
```

## Instalação

### Compilar

```powershell
cargo build --release
```

O executável será criado em:

```text
target\release\codex-usage-tray.exe
```

### Iniciar com o Windows

Pressione `Win+R`, digite `shell:startup` e coloque um atalho do `codex-usage-tray.exe` nessa pasta.

## Configuração

O arquivo fica em:

```text
%APPDATA%\codex-usage-tray\config.json
```

Ele é criado automaticamente na primeira execução.

| Campo | Padrão | Descrição |
|---|---:|---|
| `language` | `"auto"` | `"pt"`, `"en"` ou `"auto"` |
| `poll_interval_secs` | `180` | Intervalo entre consultas, mínimo efetivo de 60 s |
| `request_timeout_secs` | `20` | Timeout para handshake + leitura do app-server |
| `notify_thresholds` | `[50,80,95]` | Percentuais que disparam notificações |
| `notify_on_reset` | `true` | Notifica quando o uso cai indicando nova janela |
| `icon_yellow_at` | `60` | Percentual em que o anel fica amarelo |
| `icon_red_at` | `85` | Percentual em que o anel fica vermelho |
| `codex_command` | `"codex"` | Comando ou caminho do Codex CLI |

## Arquitetura

```text
src/
├─ app.rs       bandeja, menu e polling
├─ config.rs    configuração local
├─ i18n.rs      português/inglês
├─ icon.rs      desenho do anel
├─ notify.rs    notificações do Windows
├─ state.rs     tooltip, limiares e detecção de reset
└─ usage.rs     cliente JSON-RPC do `codex app-server`
```

## Observações

- Os dados exibidos são exatamente os buckets que o backend do Codex fornecer. Nem toda conta/plano necessariamente retorna duas janelas.
- O app prefere `rateLimitsByLimitId.codex` quando disponível e usa `rateLimits` como fallback de compatibilidade.
- O app-server é uma interface do Codex e pode evoluir. O parser evita depender da ordem `primary`/`secondary`, mas mudanças de protocolo podem exigir atualização.
- O ícone e as notificações não alteram nem consomem resets acumulados; o projeto apenas lê o estado de uso.

## Licença

MIT
