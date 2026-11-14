// Retium landing page interaction and entrance transition
(() => {
    'use strict';

    const APP_ROUTE = 'app.html';
    const TRANSITION_DURATION = 1800;

    const body = document.body;
    const cta = document.getElementById('cta');
    const ctaLabel = cta ? cta.querySelector('.cta-label') : null;
    const domainItems = Array.from(document.querySelectorAll('.domain-item'));

    const canHover = window.matchMedia('(hover: hover) and (pointer: fine)');
    const PARALLAX_RADIUS = 230;
    const PARALLAX_STRENGTH = 16;

    let pointer = null;
    let parallaxFrame = null;

    let transitioning = false;
    let navigationTimer = null;
    let convergeAnimations = [];

    const prefersReducedMotion = () =>
        window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    // Chips lean toward the cursor when it comes close, then settle back.
    function resetParallax() {
        pointer = null;

        if (parallaxFrame !== null) {
            cancelAnimationFrame(parallaxFrame);
            parallaxFrame = null;
        }

        domainItems.forEach(item => {
            item.style.setProperty('--px', '0px');
            item.style.setProperty('--py', '0px');
        });
    }

    function scheduleParallax(event) {
        if (!canHover.matches || prefersReducedMotion()) return;

        pointer = { x: event.clientX, y: event.clientY };

        if (parallaxFrame === null) {
            parallaxFrame = requestAnimationFrame(applyParallax);
        }
    }

    function applyParallax() {
        parallaxFrame = null;

        if (!pointer || transitioning || prefersReducedMotion()) return;

        domainItems.forEach(item => {
            if (item.getClientRects().length === 0) {
                item.style.setProperty('--px', '0px');
                item.style.setProperty('--py', '0px');
                return;
            }

            const rect = item.getBoundingClientRect();
            const x = rect.left + rect.width / 2;
            const y = rect.top + rect.height / 2;
            const dx = pointer.x - x;
            const dy = pointer.y - y;
            const distance = Math.hypot(dx, dy);

            if (distance === 0 || distance > PARALLAX_RADIUS) {
                item.style.setProperty('--px', '0px');
                item.style.setProperty('--py', '0px');
                return;
            }

            const pull = (1 - distance / PARALLAX_RADIUS) * PARALLAX_STRENGTH;
            item.style.setProperty('--px', `${(dx / distance) * pull}px`);
            item.style.setProperty('--py', `${(dy / distance) * pull}px`);
        });
    }

    function handlePointerOut(event) {
        if (!event.relatedTarget) resetParallax();
    }

    function resetTransition() {
        resetParallax();
        if (navigationTimer) {
            clearTimeout(navigationTimer);
            navigationTimer = null;
        }

        transitioning = false;
        body.classList.remove('is-transitioning');

        convergeAnimations.forEach(animation => animation.cancel());
        convergeAnimations = [];

        if (!cta) return;

        cta.removeAttribute('aria-busy');
        cta.style.pointerEvents = '';
        if (ctaLabel) ctaLabel.textContent = 'REGISTER YOUR NAME';

        domainItems.forEach(item => {
            item.style.animation = '';
            item.style.transform = '';
            item.style.translate = '';
            item.style.opacity = '';
        });
    }

    function convergeDomains() {
        const centerX = window.innerWidth / 2;
        const centerY = window.innerHeight / 2;

        domainItems.forEach((item, index) => {
            if (getComputedStyle(item).display === 'none') return;

            const rect = item.getBoundingClientRect();
            const x = rect.left + rect.width / 2;
            const y = rect.top + rect.height / 2;

            // Web Animations API composes over the CSS drift animation, so the
            // chip glides inward and fades instead of snapping to the center.
            const animation = item.animate(
                [
                    { transform: 'translate(-50%, -50%)', opacity: 1 },
                    {
                        transform:
                            `translate(-50%, -50%) translate(${centerX - x}px, ${centerY - y}px) scale(0.55)`,
                        opacity: 0
                    }
                ],
                {
                    duration: 1450,
                    delay: index * 15,
                    easing: 'cubic-bezier(0.33, 1, 0.68, 1)',
                    fill: 'forwards'
                }
            );

            convergeAnimations.push(animation);
        });
    }

    function enterApp(event) {
        if (transitioning) {
            event.preventDefault();
            return;
        }

        // Let browser shortcuts (new tab, download, etc.) behave normally.
        if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;

        event.preventDefault();
        transitioning = true;
        resetParallax();

        if (cta) {
            cta.setAttribute('aria-busy', 'true');
            cta.style.pointerEvents = 'none';
            if (ctaLabel) ctaLabel.textContent = 'CONNECTING TO RETIUM...';
        }

        if (prefersReducedMotion()) {
            body.classList.add('is-transitioning');
            navigationTimer = setTimeout(() => {
                window.location.assign(APP_ROUTE);
            }, 120);
            return;
        }

        convergeDomains();
        body.classList.add('is-transitioning');

        navigationTimer = setTimeout(() => {
            window.location.assign(APP_ROUTE);
        }, TRANSITION_DURATION);
    }

    if (cta) {
        cta.addEventListener('click', enterApp);
    }

    if (canHover.matches) {
        window.addEventListener('pointermove', scheduleParallax, { passive: true });
        window.addEventListener('pointerout', handlePointerOut);
        window.addEventListener('blur', resetParallax);
    }

    // Restore the landing page when the browser restores it from the bfcache.
    window.addEventListener('pageshow', (event) => {
        if (event.persisted) resetTransition();
    });

    // Clear pending timers if the page is hidden or unloaded mid-transition.
    window.addEventListener('pagehide', () => {
        if (navigationTimer) {
            clearTimeout(navigationTimer);
            navigationTimer = null;
        }
    });
})();
